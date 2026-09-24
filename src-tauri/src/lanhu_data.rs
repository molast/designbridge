use super::*;

pub(crate) fn api_url(route: &LanhuRoute) -> Result<Url, String> {
    let project_id = route
        .project_id
        .as_deref()
        .ok_or_else(|| "链接中缺少项目 ID".to_string())?;
    let mut url = Url::parse(if route.image_id.is_some() {
        "https://lanhuapp.com/api/project/image"
    } else {
        "https://lanhuapp.com/api/project/images"
    })
    .map_err(|error| format!("蓝湖接口地址无效：{error}"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("pid", project_id);
        query.append_pair("project_id", project_id);
        query.append_pair("dds_status", "1");
        if let Some(team_id) = route.team_id.as_deref() {
            query.append_pair("team_id", team_id);
        }
        if let Some(child) = route.child.as_deref() {
            query.append_pair("child", child);
        }
        if let Some(image_id) = route.image_id.as_deref() {
            query.append_pair("image_id", image_id);
            query.append_pair("comment", "1");
        } else {
            query.append_pair("position", "1");
            query.append_pair("show_cb_src", "1");
            query.append_pair("comment", "1");
        }
    }
    Ok(url)
}

pub(crate) async fn request_lanhu_json(
    client: &Client,
    url: Url,
) -> Result<serde_json::Value, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("蓝湖接口请求失败：{error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取蓝湖接口响应失败：{error}"))?;
    if !status.is_success() {
        return Err(format!("蓝湖接口返回 HTTP {status}"));
    }
    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|_| "蓝湖接口返回的不是 JSON，可能尚未完成登录".to_string())?;
    if let Some(code) = value.get("code") {
        let success = code
            .as_str()
            .map(|value| value == "00000" || value == "0")
            .or_else(|| code.as_i64().map(|value| value == 0))
            .unwrap_or(false);
        if !success {
            let message = value
                .get("msg")
                .or_else(|| value.get("message"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("蓝湖未授权当前请求");
            return Err(message.to_string());
        }
    }
    Ok(value)
}

pub(crate) fn response_data(value: &serde_json::Value) -> &serde_json::Value {
    value
        .get("data")
        .or_else(|| value.get("result"))
        .unwrap_or(value)
}

pub(crate) fn value_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(found) = object.get(*key).and_then(serde_json::Value::as_str) {
            if !found.trim().is_empty() {
                return Some(found.trim().to_string());
            }
        }
    }
    None
}

pub(crate) fn value_number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(number) = object.get(*key).and_then(serde_json::Value::as_f64) {
            if number.is_finite() && number > 0.0 {
                return Some(number);
            }
        }
        if let Some(number) = object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(|value| value.parse::<f64>().ok())
        {
            if number.is_finite() && number > 0.0 {
                return Some(number);
            }
        }
    }
    None
}

pub(crate) fn finite_number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    let object = value.as_object()?;
    for key in keys {
        let number = object.get(*key).and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
        });
        if let Some(number) = number.filter(|number| number.is_finite()) {
            return Some(number);
        }
    }
    None
}

pub(crate) fn normalize_https_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let candidate = if raw.starts_with("//") {
        format!("https:{raw}")
    } else if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else if raw.contains('/') && raw.contains('.') {
        // Lanhu sometimes stores json_url as `host/path` and adds the scheme in WebView.
        format!("https://{raw}")
    } else {
        return None;
    };
    Url::parse(&candidate)
        .ok()
        .filter(|url| url.scheme() == "https")
        .map(|_| candidate)
}

pub(crate) fn nested_url(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    if let Some(raw) = value_string(value, keys) {
        if let Some(url) = normalize_https_url(&raw) {
            return Some(url);
        }
    }
    if let Some(object) = value.as_object() {
        for child in object.values() {
            if let Some(url) = nested_url(child, keys) {
                return Some(url);
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            if let Some(url) = nested_url(child, keys) {
                return Some(url);
            }
        }
    }
    None
}

pub(crate) fn design_image_url(value: &serde_json::Value) -> Option<String> {
    nested_url(
        value,
        &[
            "url",
            "image_url",
            "imageUrl",
            "original_url",
            "origin_url",
            "preview_url",
            "cover_url",
            "coverUrl",
            "src",
            "png_xxxhd",
            "png_xhdpi",
            "png",
            "original",
        ],
    )
}

pub(crate) fn string_or_number(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        let Some(found) = object.get(*key) else {
            continue;
        };
        if let Some(text) = found
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }
        if let Some(number) = found.as_i64() {
            return Some(number.to_string());
        }
        if let Some(number) = found.as_u64() {
            return Some(number.to_string());
        }
    }
    None
}

pub(crate) fn decoded_json_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    if value.is_object() {
        return Some(value.clone());
    }
    value
        .as_str()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(serde_json::Value::is_object)
}

pub(crate) fn latest_design_version(value: &serde_json::Value) -> Option<&serde_json::Value> {
    let versions = value.get("versions")?.as_array()?;
    let latest_id = string_or_number(value, &["latest_version", "latestVersion"]);
    latest_id
        .as_deref()
        .and_then(|latest_id| {
            versions.iter().find(|version| {
                string_or_number(version, &["id", "version_id", "versionId"]).as_deref()
                    == Some(latest_id)
            })
        })
        .or_else(|| versions.first())
}

pub(crate) fn comment_author(value: &serde_json::Value) -> String {
    if let Some(author) = value_string(
        value,
        &[
            "author_name",
            "authorName",
            "creator_name",
            "creatorName",
            "nickname",
            "user_name",
            "userName",
        ],
    ) {
        return author;
    }
    for key in [
        "author",
        "user",
        "creator",
        "member",
        "editor_info",
        "editorInfo",
        "create_by",
        "createBy",
    ] {
        let Some(person) = value.get(key) else {
            continue;
        };
        if let Some(author) = value_string(
            person,
            &["nickname", "name", "username", "user_name", "userName"],
        ) {
            return author;
        }
        if let Some(author) = person
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return author.to_string();
        }
    }
    "未知成员".to_string()
}

pub(crate) fn comment_resolved(value: &serde_json::Value) -> bool {
    for key in [
        "resolved",
        "is_resolved",
        "isResolved",
        "finished",
        "closed",
    ] {
        let Some(state) = value.get(key) else {
            continue;
        };
        if let Some(state) = state.as_bool() {
            return state;
        }
        if let Some(state) = state.as_i64() {
            return state != 0;
        }
    }
    string_or_number(value, &["status", "state"])
        .map(|status| {
            matches!(
                status.to_ascii_lowercase().as_str(),
                "2" | "resolved" | "finished" | "done" | "closed"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn comment_position(
    comment: &serde_json::Value,
    design: &serde_json::Value,
    source_width: Option<f64>,
    source_height: Option<f64>,
) -> (Option<f64>, Option<f64>) {
    let ext_data = comment
        .get("extData")
        .or_else(|| comment.get("ext_data"))
        .and_then(decoded_json_value);

    let percent_x = finite_number(comment, &["positionX", "position_x"]).or_else(|| {
        ext_data
            .as_ref()
            .and_then(|ext_data| finite_number(ext_data, &["positionX", "position_x"]))
    });
    let percent_y = finite_number(comment, &["positionY", "position_y"]).or_else(|| {
        ext_data
            .as_ref()
            .and_then(|ext_data| finite_number(ext_data, &["positionY", "position_y"]))
    });
    if let (Some(percent_x), Some(percent_y), Some(width), Some(height)) =
        (percent_x, percent_y, source_width, source_height)
    {
        if (0.0..=1.0).contains(&percent_x) && (0.0..=1.0).contains(&percent_y) {
            return (Some(percent_x * width), Some(percent_y * height));
        }
    }

    let Some(position) = ext_data
        .as_ref()
        .and_then(|ext_data| ext_data.get("position"))
        .and_then(decoded_json_value)
    else {
        return (None, None);
    };
    let Some(absolute_x) = finite_number(&position, &["x", "left"]) else {
        return (None, None);
    };
    let Some(absolute_y) = finite_number(&position, &["y", "top"]) else {
        return (None, None);
    };
    let origin_x = finite_number(design, &["position_x", "positionX", "x"]).unwrap_or(0.0);
    let origin_y = finite_number(design, &["position_y", "positionY", "y"]).unwrap_or(0.0);

    let within_source = |x: f64, y: f64| match (source_width, source_height) {
        (Some(width), Some(height)) => x >= 0.0 && y >= 0.0 && x <= width && y <= height,
        _ => x >= 0.0 && y >= 0.0,
    };
    let local_x = absolute_x - origin_x;
    let local_y = absolute_y - origin_y;
    if within_source(local_x, local_y) {
        (Some(local_x), Some(local_y))
    } else if within_source(absolute_x, absolute_y) {
        (Some(absolute_x), Some(absolute_y))
    } else {
        (None, None)
    }
}

pub(crate) fn comment_target(value: &serde_json::Value) -> (Option<String>, Option<String>) {
    let ext_data = value
        .get("extData")
        .or_else(|| value.get("ext_data"))
        .and_then(decoded_json_value);
    let direct_target_id =
        string_or_number(value, &["resource_id", "resourceId", "layer_id", "layerId"]);
    let ext_target_id = ext_data.as_ref().and_then(|ext_data| {
        string_or_number(
            ext_data,
            &[
                "image_id",
                "imageId",
                "resource_id",
                "resourceId",
                "layer_id",
                "layerId",
            ],
        )
    });
    let target_type = string_or_number(
        value,
        &["resource_type", "resourceType", "target_type", "targetType"],
    )
    .or_else(|| {
        ext_data.as_ref().and_then(|ext_data| {
            string_or_number(
                ext_data,
                &["resource_type", "resourceType", "target_type", "targetType"],
            )
        })
    });
    let target_id = if target_type.as_deref() == Some("3") {
        ext_target_id.or(direct_target_id)
    } else {
        direct_target_id.or(ext_target_id)
    };
    (target_id, target_type)
}

pub(crate) fn comment_replies(value: &serde_json::Value) -> Vec<DesignCommentReply> {
    let replies = find_array(
        value,
        &["replies", "replys", "reply_list", "replyList", "children"],
    );
    replies
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, reply)| {
            let content = value_string(
                reply,
                &["content", "comment", "text", "message", "description"],
            )?;
            Some(DesignCommentReply {
                id: string_or_number(reply, &["id", "comment_id", "commentId"])
                    .unwrap_or_else(|| format!("reply-{}", index + 1)),
                author: comment_author(reply),
                content,
                created_at: string_or_number(
                    reply,
                    &[
                        "create_time",
                        "createTime",
                        "created_at",
                        "createdAt",
                        "time",
                    ],
                ),
            })
        })
        .collect()
}

pub(crate) fn collect_design_comments(value: &serde_json::Value) -> Vec<DesignComment> {
    let version = latest_design_version(value);
    let version_id =
        version.and_then(|version| string_or_number(version, &["id", "version_id", "versionId"]));
    let version_name = version.and_then(|version| {
        value_string(
            version,
            &[
                "version_info",
                "versionInfo",
                "name",
                "version_name",
                "versionName",
            ],
        )
    });
    let source_width = version
        .and_then(|version| value_number(version, &["width", "w"]))
        .or_else(|| value_number(value, &["width", "w"]));
    let source_height = version
        .and_then(|version| value_number(version, &["height", "h"]))
        .or_else(|| value_number(value, &["height", "h"]));
    let comments = version
        .and_then(|version| find_array(version, &["comments", "comment_list", "commentList"]))
        .or_else(|| find_array(value, &["comments", "comment_list", "commentList"]));

    comments
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(position, comment)| {
            let content = value_string(
                comment,
                &["content", "comment", "text", "message", "description"],
            )?;
            let index = string_or_number(
                comment,
                &[
                    "index",
                    "number",
                    "order",
                    "sequence",
                    "serial_number",
                    "serialNumber",
                    "text",
                ],
            )
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(position + 1);
            let (x, y) = comment_position(comment, value, source_width, source_height);
            let (target_id, target_type) = comment_target(comment);
            Some(DesignComment {
                id: string_or_number(
                    comment,
                    &["id", "comment_id", "commentId", "anchor_uuid", "anchorUuid"],
                )
                .unwrap_or_else(|| format!("comment-{index}")),
                index,
                author: comment_author(comment),
                content,
                created_at: string_or_number(
                    comment,
                    &[
                        "create_time",
                        "createTime",
                        "created_at",
                        "createdAt",
                        "time",
                    ],
                ),
                resolved: comment_resolved(comment),
                x,
                y,
                source_width,
                source_height,
                version_id: version_id.clone(),
                version_name: version_name.clone(),
                target_id,
                target_type,
                replies: comment_replies(comment),
            })
        })
        .collect()
}

pub(crate) fn design_from_value(
    value: &serde_json::Value,
    fallback_id: &str,
) -> Option<LanhuDesignPayload> {
    let version = latest_design_version(value).unwrap_or(&serde_json::Value::Null);
    let url = design_image_url(value).or_else(|| design_image_url(version))?;
    let comments = collect_design_comments(value);
    Some(LanhuDesignPayload {
        id: value_string(value, &["id", "image_id", "imageId", "web_id"])
            .unwrap_or_else(|| fallback_id.to_string()),
        name: value_string(value, &["name", "title"]).unwrap_or_else(|| "未命名画板".to_string()),
        width: value_number(value, &["width", "w"]),
        height: value_number(value, &["height", "h"]),
        coordinate_space: None,
        url,
        update_time: value_string(value, &["update_time", "updateTime"]),
        has_comment: value
            .get("has_comment")
            .or_else(|| value.get("hasComment"))
            .and_then(serde_json::Value::as_bool),
        comments,
    })
}

pub(crate) fn android_coordinate_space(value: &serde_json::Value) -> Option<DesignCoordinateSpace> {
    let artboard = value.get("artboard")?;
    let frame = artboard
        .get("frame")
        .or_else(|| artboard.get("realFrame"))?;
    let width = value_number(frame, &["width", "w"])?;
    let height = value_number(frame, &["height", "h"])?;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }

    Some(DesignCoordinateSpace {
        platform: "android".to_string(),
        width,
        height,
        unit: "dp".to_string(),
    })
}

pub(crate) fn layer_frame(value: &serde_json::Value) -> Option<LayerFrame> {
    let frame = value.get("realFrame").or_else(|| value.get("frame"))?;
    let width = finite_number(frame, &["width", "w"])?;
    let height = finite_number(frame, &["height", "h"])?;
    if width < 0.0 || height < 0.0 {
        return None;
    }
    Some(LayerFrame {
        x: finite_number(frame, &["left", "x"]).unwrap_or(0.0),
        y: finite_number(frame, &["top", "y"]).unwrap_or(0.0),
        width,
        height,
    })
}

pub(crate) fn radius_value(value: &serde_json::Value) -> Option<LayerRadius> {
    let radius = LayerRadius {
        top_left: finite_number(value, &["topLeft", "top_left"]),
        top_right: finite_number(value, &["topRight", "top_right"]),
        bottom_right: finite_number(value, &["bottomRight", "bottom_right"]),
        bottom_left: finite_number(value, &["bottomLeft", "bottom_left"]),
    };
    (radius.top_left.is_some()
        || radius.top_right.is_some()
        || radius.bottom_right.is_some()
        || radius.bottom_left.is_some())
    .then_some(radius)
}

pub(crate) fn layer_radius(value: &serde_json::Value) -> LayerRadius {
    let path_radius = value
        .get("paths")
        .and_then(serde_json::Value::as_array)
        .and_then(|paths| {
            paths
                .iter()
                .find_map(|path| path.get("radius").and_then(radius_value))
        });
    path_radius
        .or_else(|| value.get("radius").and_then(radius_value))
        .unwrap_or_default()
}

pub(crate) fn color_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("color")
        .and_then(|color| value_string(color, &["value", "hex"]))
        .or_else(|| value_string(value, &["value", "hex"]))
}

pub(crate) fn color_token(value: &serde_json::Value) -> Option<String> {
    value
        .pointer("/boundVariables/color/name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .pointer("/color/boundVariables/color/name")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
}

pub(crate) fn layer_paints(value: &serde_json::Value) -> Vec<LayerPaint> {
    value
        .pointer("/style/fills")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|fill| {
            fill.get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|fill| LayerPaint {
            paint_type: value_string(fill, &["type"]).unwrap_or_else(|| "color".to_string()),
            color: color_value(fill),
            token: color_token(fill),
            opacity: finite_number(fill, &["opacity"]).unwrap_or(1.0),
        })
        .collect()
}

pub(crate) fn layer_borders(value: &serde_json::Value) -> Vec<LayerBorder> {
    value
        .pointer("/style/borders")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|border| {
            border
                .get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|border| LayerBorder {
            width: finite_number(border, &["width"]).unwrap_or(0.0),
            style: value_string(border, &["style"]).unwrap_or_else(|| "solid".to_string()),
            color: color_value(border),
            token: color_token(border),
            opacity: finite_number(border, &["opacity"]).unwrap_or(1.0),
        })
        .collect()
}

pub(crate) fn layer_shadows(value: &serde_json::Value) -> Vec<LayerShadow> {
    value
        .pointer("/style/shadows")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|shadow| {
            shadow
                .get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|shadow| {
            let offset = shadow.get("offset").unwrap_or(shadow);
            LayerShadow {
                shadow_type: value_string(shadow, &["type"])
                    .unwrap_or_else(|| "dropShadow".to_string()),
                color: color_value(shadow),
                offset_x: finite_number(offset, &["x", "left"]).unwrap_or(0.0),
                offset_y: finite_number(offset, &["y", "top"]).unwrap_or(0.0),
                blur: finite_number(shadow, &["blur", "radius"]).unwrap_or(0.0),
                spread: finite_number(shadow, &["spread"]).unwrap_or(0.0),
            }
        })
        .collect()
}

pub(crate) fn layer_blurs(value: &serde_json::Value) -> Vec<LayerBlur> {
    value
        .pointer("/style/blurs")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|blur| {
            blur.get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|blur| LayerBlur {
            blur_type: value_string(blur, &["type"]).unwrap_or_else(|| "layerBlur".to_string()),
            radius: finite_number(blur, &["radius", "blur"]).unwrap_or(0.0),
        })
        .collect()
}

pub(crate) fn exact_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

pub(crate) fn text_range_index(value: &serde_json::Value, key: &str) -> Option<usize> {
    finite_number(value, &[key])
        .filter(|index| *index >= 0.0 && index.fract() == 0.0)
        .map(|index| index as usize)
}

pub(crate) fn text_metric(font: &serde_json::Value, key: &str) -> (Option<f64>, Option<String>) {
    let Some(metric) = font.get(key) else {
        return (None, None);
    };
    match metric {
        serde_json::Value::Object(_) => (
            finite_number(metric, &["value"]),
            value_string(metric, &["unit"]),
        ),
        _ => (finite_number(font, &[key]), None),
    }
}

pub(crate) fn text_content_for_range(
    content: &str,
    from: Option<usize>,
    to: Option<usize>,
) -> String {
    let Some((from, to)) = from.zip(to) else {
        return String::new();
    };
    if to < from {
        return String::new();
    }
    content.chars().skip(from).take(to - from).collect()
}

pub(crate) fn layer_text_style(value: &serde_json::Value, full_content: &str) -> LayerTextStyle {
    let font = value.get("font").unwrap_or(value);
    let color = value.get("color").unwrap_or(value);
    let from = text_range_index(value, "from");
    let to = text_range_index(value, "to");
    let (line_height, line_height_unit) = text_metric(font, "lineHeight");
    let (letter_spacing, letter_spacing_unit) = text_metric(font, "letterSpacing");
    let content = exact_string(value, &["content"])
        .unwrap_or_else(|| text_content_for_range(full_content, from, to));

    LayerTextStyle {
        content,
        from,
        to,
        font_family: value_string(font, &["name", "fontFamily"]),
        post_script_name: value_string(font, &["postScriptName"]),
        font_style: value_string(font, &["type", "style"]),
        font_size: finite_number(font, &["size", "fontSize"]),
        font_weight: finite_number(font, &["fontWeight", "weight"]),
        alignment: value_string(font, &["align", "textAlign"]),
        vertical_alignment: value_string(font, &["verticalAlignment", "verticalAlign"]),
        line_height,
        line_height_unit,
        letter_spacing,
        letter_spacing_unit,
        color: value_string(color, &["value", "hex"]),
        token: color_token(color),
    }
}

pub(crate) fn layer_text(value: &serde_json::Value) -> Option<LayerText> {
    let text = value.get("text")?;
    let style = text.get("style").unwrap_or(text);
    let font = style.get("font").unwrap_or(style);
    let content =
        exact_string(text, &["value", "content"]).or_else(|| exact_string(style, &["content"]))?;
    let color = style.get("color").unwrap_or(style);
    let styles = text
        .get("styles")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|style| layer_text_style(style, &content))
        .filter(|style| !style.content.is_empty())
        .collect();
    Some(LayerText {
        content,
        font_family: value_string(font, &["name", "fontFamily", "postScriptName"]),
        font_size: finite_number(font, &["size", "fontSize"]),
        font_weight: finite_number(font, &["fontWeight", "weight"]),
        alignment: value_string(font, &["align", "textAlign"]),
        line_height: font
            .get("lineHeight")
            .and_then(|line_height| finite_number(line_height, &["value"]))
            .or_else(|| finite_number(font, &["lineHeight"])),
        letter_spacing: font
            .get("letterSpacing")
            .and_then(|spacing| finite_number(spacing, &["value"]))
            .or_else(|| finite_number(font, &["letterSpacing"])),
        color: value_string(color, &["value", "hex"]),
        token: color_token(color),
        styles,
    })
}

pub(crate) fn collect_layers(value: &serde_json::Value) -> Vec<InspectableLayer> {
    fn visit(
        value: &serde_json::Value,
        parent_id: Option<&str>,
        depth: usize,
        is_root: bool,
        order: &mut usize,
        output: &mut Vec<InspectableLayer>,
    ) {
        if depth > 64 || output.len() >= 20_000 {
            return;
        }

        let id = value_string(value, &["id", "web_id", "webId"]);
        if let Some(id) = id.as_deref() {
            let mut frame = layer_frame(value);
            if is_root {
                if let Some(frame) = frame.as_mut() {
                    frame.x = 0.0;
                    frame.y = 0.0;
                }
            }
            output.push(InspectableLayer {
                id: id.to_string(),
                parent_id: parent_id.map(str::to_string),
                name: value_string(value, &["name", "title"])
                    .unwrap_or_else(|| "未命名图层".to_string()),
                layer_type: value_string(value, &["type"]).unwrap_or_else(|| "layer".to_string()),
                depth,
                order: *order,
                frame,
                frame_is_visual: value.get("realFrame").is_some(),
                opacity: finite_number(value, &["opacity"]).unwrap_or(1.0),
                rotation: finite_number(value, &["rotation"]).unwrap_or(0.0),
                visible: value
                    .get("visible")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
                pass_through: value
                    .get("isPassThrough")
                    .or_else(|| value.get("passThrough"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                radius: layer_radius(value),
                fills: layer_paints(value),
                borders: layer_borders(value),
                shadows: layer_shadows(value),
                blurs: layer_blurs(value),
                text: layer_text(value),
                is_asset: value
                    .get("isAsset")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                    || value
                        .get("hasExportImage")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    || value.get("type").and_then(serde_json::Value::as_str) == Some("bitmapLayer"),
                has_slice: false,
            });
            *order += 1;
        }

        if let Some(children) = value.get("layers").and_then(serde_json::Value::as_array) {
            for child in children {
                visit(
                    child,
                    id.as_deref().or(parent_id),
                    depth + usize::from(id.is_some()),
                    false,
                    order,
                    output,
                );
            }
        }
    }

    let Some(artboard) = value.get("artboard") else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut order = 0;
    visit(artboard, None, 0, true, &mut order, &mut output);
    output
}

pub(crate) fn link_slices_to_layers(layers: &mut [InspectableLayer], slices: &[LanhuSlicePayload]) {
    let slice_ids = slices
        .iter()
        .map(|slice| slice.id.as_str())
        .collect::<HashSet<_>>();
    for layer in layers {
        layer.has_slice = slice_ids.contains(layer.id.as_str());
        layer.is_asset |= layer.has_slice;
    }
}

pub(crate) fn align_layer_bound_comments(
    designs: &mut [LanhuDesignPayload],
    layers: &[InspectableLayer],
) {
    for design in designs {
        let Some(coordinate_space) = design.coordinate_space.as_ref() else {
            continue;
        };
        for comment in &mut design.comments {
            let Some(target_id) = comment.target_id.as_deref() else {
                continue;
            };
            if target_id == design.id {
                continue;
            }
            let Some(frame) = layers
                .iter()
                .find(|layer| layer.id == target_id)
                .and_then(|layer| layer.frame.as_ref())
            else {
                continue;
            };
            let (Some(x), Some(y), Some(source_width), Some(source_height)) = (
                comment.x,
                comment.y,
                comment.source_width,
                comment.source_height,
            ) else {
                continue;
            };
            if source_width <= 0.0 || source_height <= 0.0 {
                continue;
            }
            comment.x = Some(frame.x + (x / source_width) * frame.width);
            comment.y = Some(frame.y + (y / source_height) * frame.height);
            comment.source_width = Some(coordinate_space.width);
            comment.source_height = Some(coordinate_space.height);
        }
    }
}

pub(crate) fn normalize_legacy_layer_frames(layers: &mut [InspectableLayer]) {
    for layer in layers {
        if layer.frame_is_visual {
            continue;
        }
        layer.frame_is_visual = true;

        let Some(frame) = layer.frame.as_mut() else {
            continue;
        };
        let rotation = layer.rotation.rem_euclid(360.0);
        if rotation.abs() < 0.0001 || (rotation - 360.0).abs() < 0.0001 {
            continue;
        }

        // Figma represents a horizontal flip as a 180-degree rotation while Lanhu's
        // visual frame only moves to the opposite horizontal edge.
        if (rotation - 180.0).abs() < 0.0001 {
            frame.x -= frame.width;
            continue;
        }

        let radians = rotation.to_radians();
        let cosine = radians.cos();
        let sine = radians.sin();
        let corners = [
            (0.0, 0.0),
            (frame.width, 0.0),
            (0.0, frame.height),
            (frame.width, frame.height),
        ];
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in corners {
            let rotated_x = x * cosine + y * sine;
            let rotated_y = -x * sine + y * cosine;
            min_x = min_x.min(rotated_x);
            min_y = min_y.min(rotated_y);
            max_x = max_x.max(rotated_x);
            max_y = max_y.max(rotated_y);
        }
        frame.x += min_x;
        frame.y += min_y;
        frame.width = max_x - min_x;
        frame.height = max_y - min_y;
    }
}

pub(crate) fn find_array<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a Vec<serde_json::Value>> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(array) = object.get(*key).and_then(serde_json::Value::as_array) {
            return Some(array);
        }
    }
    None
}

pub(crate) fn find_array_recursive<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a Vec<serde_json::Value>> {
    if let Some(array) = find_array(value, keys) {
        return Some(array);
    }
    if let Some(object) = value.as_object() {
        for child in object.values() {
            if let Some(array) = find_array_recursive(child, keys) {
                return Some(array);
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            if let Some(found) = find_array_recursive(child, keys) {
                return Some(found);
            }
        }
    }
    None
}

pub(crate) fn project_from_response(
    value: &serde_json::Value,
    route: &LanhuRoute,
    resolved_url: &str,
) -> Result<LanhuProjectPayload, String> {
    let data = response_data(value);
    let mut designs = Vec::new();
    if let Some(image_id) = route.image_id.as_deref() {
        let candidate = design_from_value(data, image_id).or_else(|| {
            data.as_object().and_then(|object| {
                object
                    .values()
                    .find_map(|child| design_from_value(child, image_id))
            })
        });
        if let Some(design) = candidate {
            designs.push(design);
        }
    } else if let Some(array) = find_array_recursive(
        data,
        &["images", "designs", "items", "list", "pages", "children"],
    ) {
        designs.extend(
            array
                .iter()
                .enumerate()
                .filter_map(|(index, item)| design_from_value(item, &format!("design-{index}"))),
        );
    }
    if designs.is_empty() {
        return Err("蓝湖接口暂未返回可下载的画板".to_string());
    }

    Ok(LanhuProjectPayload {
        resolved_url: resolved_url.to_string(),
        team_id: route
            .team_id
            .clone()
            .or_else(|| value_string(data, &["team_id", "teamId"]))
            .unwrap_or_default(),
        project_id: route.project_id.clone().unwrap_or_default(),
        project_name: value_string(data, &["name", "project_name", "projectName"])
            .unwrap_or_else(|| "未命名项目".to_string()),
        designs,
        slices: Vec::new(),
        layers: Vec::new(),
    })
}

pub(crate) fn find_json_url(value: &serde_json::Value) -> Option<String> {
    nested_url(value, &["jsonurl", "json_url", "jsonUrl"])
}

pub(crate) fn slice_image_url(value: &serde_json::Value) -> Option<String> {
    nested_url(
        value,
        &[
            "png_xxxhd",
            "png_xhdpi",
            "png_hdpi",
            "bitmap",
            "image_url",
            "imageUrl",
            "orgUrl",
            "org_url",
            "url",
        ],
    )
}

pub(crate) fn collect_slices(value: &serde_json::Value) -> Vec<LanhuSlicePayload> {
    fn visit(
        value: &serde_json::Value,
        output: &mut Vec<LanhuSlicePayload>,
        seen: &mut HashSet<String>,
        depth: usize,
    ) {
        if depth > 64 || output.len() >= 20_000 {
            return;
        }
        if let Some(object) = value.as_object() {
            let is_asset = object
                .get("isAsset")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                || object
                    .get("exportable")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                || object.contains_key("exportable")
                || object
                    .get("slice")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                || object
                    .get("image")
                    .and_then(|item| item.get("bitmap"))
                    .is_some()
                || object.get("type").and_then(serde_json::Value::as_str) == Some("bitmapLayer")
                || object
                    .get("image")
                    .and_then(|item| item.get("imageUrl"))
                    .is_some()
                || object
                    .get("images")
                    .and_then(|item| item.get("png_xxxhd"))
                    .is_some();
            if is_asset {
                if let Some(url) = slice_image_url(value) {
                    let name = value_string(value, &["name", "title"])
                        .unwrap_or_else(|| "untitled".to_string());
                    let id = value_string(value, &["web_id", "webId", "id"])
                        .unwrap_or_else(|| format!("slice-{}", output.len()));
                    let key = format!("{id}\n{name}\n{url}");
                    if seen.insert(key) {
                        output.push(LanhuSlicePayload {
                            id,
                            name,
                            width: value_number(value, &["width", "w"]),
                            height: value_number(value, &["height", "h"]),
                            url,
                            org_url: nested_url(value, &["orgUrl", "org_url"]),
                            svg_url: nested_url(value, &["svg", "svgUrl", "svg_url"]),
                        });
                    }
                }
            }
            for child in object.values() {
                visit(child, output, seen, depth + 1);
            }
        } else if let Some(array) = value.as_array() {
            for child in array {
                visit(child, output, seen, depth + 1);
            }
        }
    }

    let mut output = Vec::new();
    let mut seen = HashSet::new();
    visit(value, &mut output, &mut seen, 0);
    output
}
