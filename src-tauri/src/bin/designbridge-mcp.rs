use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
#[cfg(test)]
use designbridge_lib::design_data::DesignCoordinateSpace;
use designbridge_lib::design_data::{
    CaptureResult, CapturedDesign, CapturedSlice, DesignComment, InspectableLayer, LayerFrame,
};
use image::GenericImageView;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    io::Cursor,
    path::{Path, PathBuf},
};
use url::Url;

const MAX_TOOL_LAYERS: usize = 500;
const MAX_TOOL_ASSETS: usize = 24;
const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone)]
struct DesignBridgeMcp {
    captures_root: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListDesignsRequest {
    /// Maximum number of recent unique designs to return. Defaults to 30 and is capped at 100.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ResolveDesignRequest {
    /// A designbridge:// design link or the original Lanhu design URL.
    link: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RectInput {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl RectInput {
    fn validate(&self) -> Result<LayerFrame, String> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Err(
                "rect must contain finite x/y values and positive width/height".to_string(),
            );
        }
        Ok(LayerFrame {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        })
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GetDesignContextRequest {
    /// A designbridge:// design link or the original Lanhu design URL.
    link: String,
    /// Optional layer id. Overrides node-id from the link when supplied.
    node_id: Option<String>,
    /// Optional design-coordinate rectangle used to select intersecting layers.
    rect: Option<RectInput>,
    /// Maximum descendant depth for node queries or root depth for whole-design queries. Defaults to 3.
    max_depth: Option<usize>,
    /// Include hidden layers. Defaults to false.
    include_hidden: Option<bool>,
    /// Maximum returned layers. Defaults to 200 and is capped at 500.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FindLayersRequest {
    /// A designbridge:// design link or the original Lanhu design URL.
    link: String,
    /// X coordinate in the design coordinate space. Must be supplied together with y.
    x: Option<f64>,
    /// Y coordinate in the design coordinate space. Must be supplied together with x.
    y: Option<f64>,
    /// Optional rectangle in design coordinates.
    rect: Option<RectInput>,
    /// Optional case-insensitive text matched against layer names and text content.
    text: Option<String>,
    /// Maximum matches. Defaults to 40 and is capped at 200.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GetDesignScreenshotRequest {
    /// A designbridge:// design link or the original Lanhu design URL.
    link: String,
    /// Optional crop rectangle in design coordinates. When omitted, returns the full design image.
    rect: Option<RectInput>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GetAssetsRequest {
    /// A designbridge:// design link or the original Lanhu design URL.
    link: String,
    /// Layer ids whose linked slices should be returned. When omitted, returns slices for the link's node-id or all slices.
    node_ids: Option<Vec<String>>,
    /// Include image content in the MCP response. Defaults to true.
    include_images: Option<bool>,
    /// Maximum assets. Defaults to 12 and is capped at 24.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GetCommentsRequest {
    /// A designbridge:// design link or the original Lanhu design URL.
    link: String,
    /// Optional comment id. When omitted, returns every comment on the design.
    comment_id: Option<String>,
    /// Optional design-coordinate rectangle. When supplied, only comments inside this UI region are returned.
    rect: Option<RectInput>,
}

#[derive(Clone, Debug)]
struct DesignReference {
    project_id: String,
    image_id: String,
    node_id: Option<String>,
    rect: Option<LayerFrame>,
}

#[derive(Clone, Debug)]
struct ResolvedDesign {
    capture: CaptureResult,
    design: CapturedDesign,
}

impl DesignBridgeMcp {
    fn new(captures_root: PathBuf) -> Self {
        Self {
            captures_root,
            tool_router: Self::tool_router(),
        }
    }

    async fn load_captures(&self) -> Result<Vec<CaptureResult>, String> {
        let mut directory = match tokio::fs::read_dir(&self.captures_root).await {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("cannot read DesignBridge captures: {error}")),
        };
        let mut captures = Vec::new();
        while let Some(entry) = directory
            .next_entry()
            .await
            .map_err(|error| format!("cannot enumerate DesignBridge captures: {error}"))?
        {
            let path = entry.path().join("capture.json");
            let Ok(bytes) = tokio::fs::read(path).await else {
                continue;
            };
            if let Ok(capture) = serde_json::from_slice::<CaptureResult>(&bytes) {
                captures.push(capture);
            }
        }
        captures.sort_by(|left, right| right.captured_at.cmp(&left.captured_at));
        Ok(captures)
    }

    async fn resolve(&self, link: &str) -> Result<(ResolvedDesign, DesignReference), String> {
        let reference = parse_design_reference(link)?;
        let captures = self.load_captures().await?;
        for capture in captures {
            if capture.project_id != reference.project_id {
                continue;
            }
            if let Some(design) = capture
                .designs
                .iter()
                .find(|design| design.id == reference.image_id)
                .cloned()
            {
                return Ok((ResolvedDesign { capture, design }, reference));
            }
        }
        Err(format!(
            "design not found in local captures: project_id={}, image_id={}. Capture it in DesignBridge first.",
            reference.project_id, reference.image_id
        ))
    }
}

#[tool_router]
impl DesignBridgeMcp {
    #[tool(
        description = "List locally captured DesignBridge designs and their stable designbridge:// links. Call this when the user has not supplied a link.",
        annotations(title = "List DesignBridge designs", read_only_hint = true)
    )]
    async fn list_designs(
        &self,
        Parameters(request): Parameters<ListDesignsRequest>,
    ) -> Result<CallToolResult, String> {
        let limit = request.limit.unwrap_or(30).clamp(1, 100);
        let captures = self.load_captures().await?;
        let mut seen = HashSet::new();
        let mut designs = Vec::new();
        for capture in captures {
            for design in &capture.designs {
                let key = (capture.project_id.clone(), design.id.clone());
                if !seen.insert(key) {
                    continue;
                }
                designs.push(design_summary(&capture, design));
                if designs.len() >= limit {
                    return Ok(CallToolResult::structured(json!({ "designs": designs })));
                }
            }
        }
        Ok(CallToolResult::structured(json!({ "designs": designs })))
    }

    #[tool(
        description = "Resolve a designbridge:// or Lanhu link to the latest matching local capture, including its frame, image path, layer count, and slice count.",
        annotations(title = "Resolve DesignBridge link", read_only_hint = true)
    )]
    async fn resolve_design(
        &self,
        Parameters(request): Parameters<ResolveDesignRequest>,
    ) -> Result<CallToolResult, String> {
        let (resolved, reference) = self.resolve(&request.link).await?;
        Ok(CallToolResult::structured(json!({
            "design": design_summary(&resolved.capture, &resolved.design),
            "requestedNodeId": reference.node_id,
            "requestedRect": reference.rect,
        })))
    }

    #[tool(
        description = "Get design metadata, exact layer frames and visual styles for a node, rectangle, or shallow whole-design tree. Prefer node-id or rect queries to keep context focused.",
        annotations(title = "Get DesignBridge design context", read_only_hint = true)
    )]
    async fn get_design_context(
        &self,
        Parameters(request): Parameters<GetDesignContextRequest>,
    ) -> Result<CallToolResult, String> {
        let (resolved, reference) = self.resolve(&request.link).await?;
        let node_id = request.node_id.or(reference.node_id);
        let rect = match request.rect {
            Some(rect) => Some(rect.validate()?),
            None => reference.rect,
        };
        let max_depth = request.max_depth.unwrap_or(3).min(20);
        let limit = request.limit.unwrap_or(200).clamp(1, MAX_TOOL_LAYERS);
        let include_hidden = request.include_hidden.unwrap_or(false);
        let all_layers = &resolved.capture.layers;
        let parent_map: HashMap<&str, Option<&str>> = all_layers
            .iter()
            .map(|layer| (layer.id.as_str(), layer.parent_id.as_deref()))
            .collect();
        let node_depth = node_id.as_deref().and_then(|id| {
            all_layers
                .iter()
                .find(|layer| layer.id == id)
                .map(|layer| layer.depth)
        });
        if node_id.is_some() && node_depth.is_none() {
            return Err(format!(
                "layer not found: {}",
                node_id.as_deref().unwrap_or_default()
            ));
        }

        let mut matched: Vec<&InspectableLayer> = all_layers
            .iter()
            .filter(|layer| include_hidden || layer.visible)
            .filter(|layer| {
                node_id.as_deref().is_none_or(|target| {
                    (layer.id == target || is_descendant_of(layer, target, &parent_map))
                        && layer.depth.saturating_sub(node_depth.unwrap_or(0)) <= max_depth
                })
            })
            .filter(|layer| {
                rect.as_ref().is_none_or(|rect| {
                    layer
                        .frame
                        .as_ref()
                        .is_some_and(|frame| frame.intersects(rect))
                })
            })
            .filter(|layer| node_id.is_some() || rect.is_some() || layer.depth <= max_depth)
            .collect();
        matched.sort_by_key(|layer| (layer.depth, layer.order));
        let total_matches = matched.len();
        matched.truncate(limit);

        let layer_ids: HashSet<&str> = matched.iter().map(|layer| layer.id.as_str()).collect();
        let layer_names: HashSet<&str> = matched
            .iter()
            .filter(|layer| layer.has_slice)
            .map(|layer| layer.name.as_str())
            .collect();
        let assets: Vec<Value> = resolved
            .capture
            .slices
            .iter()
            .filter(|slice| {
                layer_ids.contains(slice.id.as_str()) || layer_names.contains(slice.name.as_str())
            })
            .map(asset_summary)
            .collect();
        let node_frame = node_id.as_deref().and_then(|node_id| {
            all_layers
                .iter()
                .find(|layer| layer.id == node_id)
                .and_then(|layer| layer.frame.as_ref())
        });
        let comments = resolved
            .design
            .comments
            .iter()
            .filter(|comment| {
                rect.as_ref().is_none_or(|rect| {
                    comment_design_position(&resolved.design, comment)
                        .is_some_and(|(x, y)| rect.contains(x, y))
                })
            })
            .filter(|comment| {
                node_id.as_deref().is_none_or(|node_id| {
                    let targets_node = comment.target_id.as_deref().is_some_and(|target_id| {
                        target_id == node_id
                            || all_layers
                                .iter()
                                .find(|layer| layer.id == target_id)
                                .is_some_and(|layer| is_descendant_of(layer, node_id, &parent_map))
                    });
                    targets_node
                        || node_frame.is_some_and(|frame| {
                            comment_design_position(&resolved.design, comment)
                                .is_some_and(|(x, y)| frame.contains(x, y))
                        })
                })
            })
            .map(|comment| comment_summary(&resolved.capture, &resolved.design, comment))
            .collect::<Vec<_>>();

        Ok(CallToolResult::structured(json!({
            "design": design_summary(&resolved.capture, &resolved.design),
            "query": { "nodeId": node_id, "rect": rect, "maxDepth": max_depth, "includeHidden": include_hidden },
            "layers": matched,
            "assets": assets,
            "comments": comments,
            "totalMatches": total_matches,
            "truncated": total_matches > limit,
        })))
    }

    #[tool(
        description = "Find candidate layers at a point, intersecting a rectangle, or matching text. Slice layers are ranked first for point queries.",
        annotations(title = "Find DesignBridge layers", read_only_hint = true)
    )]
    async fn find_layers(
        &self,
        Parameters(request): Parameters<FindLayersRequest>,
    ) -> Result<CallToolResult, String> {
        let (resolved, _) = self.resolve(&request.link).await?;
        let point = match (request.x, request.y) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Some((x, y)),
            (None, None) => None,
            _ => return Err("x and y must be supplied together as finite numbers".to_string()),
        };
        let rect = request.rect.map(|rect| rect.validate()).transpose()?;
        let text = request
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_lowercase);
        if point.is_none() && rect.is_none() && text.is_none() {
            return Err("provide x/y, rect, or text to find layers".to_string());
        }

        let mut layers: Vec<&InspectableLayer> = resolved
            .capture
            .layers
            .iter()
            .filter(|layer| layer.visible)
            .filter(|layer| {
                point.is_none_or(|(x, y)| {
                    layer
                        .frame
                        .as_ref()
                        .is_some_and(|frame| frame.contains(x, y))
                })
            })
            .filter(|layer| {
                rect.as_ref().is_none_or(|rect| {
                    layer
                        .frame
                        .as_ref()
                        .is_some_and(|frame| frame.intersects(rect))
                })
            })
            .filter(|layer| {
                text.as_ref().is_none_or(|text| {
                    layer.name.to_lowercase().contains(text)
                        || layer
                            .text
                            .as_ref()
                            .is_some_and(|value| value.content.to_lowercase().contains(text))
                })
            })
            .collect();

        if point.is_some() {
            layers.sort_by(|left, right| {
                right
                    .has_slice
                    .cmp(&left.has_slice)
                    .then_with(|| {
                        frame_area(left)
                            .partial_cmp(&frame_area(right))
                            .unwrap_or(Ordering::Equal)
                    })
                    .then_with(|| right.depth.cmp(&left.depth))
                    .then_with(|| right.order.cmp(&left.order))
            });
        } else {
            layers.sort_by_key(|layer| (layer.depth, layer.order));
        }
        let total_matches = layers.len();
        let limit = request.limit.unwrap_or(40).clamp(1, 200);
        layers.truncate(limit);
        let summaries: Vec<Value> = layers.iter().map(|layer| layer_summary(layer)).collect();
        Ok(CallToolResult::structured(json!({
            "design": design_summary(&resolved.capture, &resolved.design),
            "layers": summaries,
            "totalMatches": total_matches,
            "truncated": total_matches > limit,
        })))
    }

    #[tool(
        description = "Return the captured design image, optionally cropped using design coordinates. Use this for visual verification after resolving the design link.",
        annotations(title = "Get DesignBridge screenshot", read_only_hint = true)
    )]
    async fn get_design_screenshot(
        &self,
        Parameters(request): Parameters<GetDesignScreenshotRequest>,
    ) -> Result<CallToolResult, String> {
        let (resolved, reference) = self.resolve(&request.link).await?;
        let path = resolved
            .design
            .local_path
            .as_deref()
            .ok_or_else(|| "the captured design does not have a local image".to_string())?;
        let crop = match request.rect {
            Some(rect) => Some(rect.validate()?),
            None => reference.rect,
        };
        let (bytes, mime_type, pixel_width, pixel_height) =
            read_design_image(Path::new(path), &resolved.design, crop.as_ref()).await?;
        let metadata = json!({
            "design": design_summary(&resolved.capture, &resolved.design),
            "crop": crop,
            "pixelWidth": pixel_width,
            "pixelHeight": pixel_height,
            "mimeType": mime_type,
        });
        let mut result = CallToolResult::success(vec![
            ContentBlock::text(metadata.to_string()),
            ContentBlock::image(BASE64.encode(bytes), mime_type),
        ]);
        result.structured_content = Some(metadata);
        Ok(result)
    }

    #[tool(
        description = "Return metadata and image content for locally downloaded slices linked to selected layer ids. Use after get_design_context or find_layers.",
        annotations(title = "Get DesignBridge assets", read_only_hint = true)
    )]
    async fn get_assets(
        &self,
        Parameters(request): Parameters<GetAssetsRequest>,
    ) -> Result<CallToolResult, String> {
        let (resolved, reference) = self.resolve(&request.link).await?;
        let requested_ids = request
            .node_ids
            .or_else(|| reference.node_id.map(|id| vec![id]))
            .unwrap_or_default();
        let requested_ids: HashSet<&str> = requested_ids.iter().map(String::as_str).collect();
        let linked_names: HashSet<&str> = resolved
            .capture
            .layers
            .iter()
            .filter(|layer| requested_ids.contains(layer.id.as_str()) && layer.has_slice)
            .map(|layer| layer.name.as_str())
            .collect();
        let mut slices: Vec<&CapturedSlice> = resolved
            .capture
            .slices
            .iter()
            .filter(|slice| {
                requested_ids.is_empty()
                    || requested_ids.contains(slice.id.as_str())
                    || linked_names.contains(slice.name.as_str())
            })
            .collect();
        let total_matches = slices.len();
        let limit = request.limit.unwrap_or(12).clamp(1, MAX_TOOL_ASSETS);
        slices.truncate(limit);

        let metadata = json!({
            "design": design_summary(&resolved.capture, &resolved.design),
            "assets": slices.iter().map(|slice| asset_summary(slice)).collect::<Vec<_>>(),
            "totalMatches": total_matches,
            "truncated": total_matches > limit,
        });
        let mut content = vec![ContentBlock::text(metadata.to_string())];
        if request.include_images.unwrap_or(true) {
            for slice in &slices {
                let Some(path) = slice.local_path.as_deref() else {
                    continue;
                };
                let bytes = read_limited(Path::new(path)).await?;
                content.push(ContentBlock::image(BASE64.encode(bytes), image_mime(path)));
            }
        }
        let mut result = CallToolResult::success(content);
        result.structured_content = Some(metadata);
        Ok(result)
    }

    #[tool(
        description = "Return comments captured from a Lanhu design, including their design coordinates, author, content, state, version, and replies.",
        annotations(title = "Get DesignBridge comments", read_only_hint = true)
    )]
    async fn get_comments(
        &self,
        Parameters(request): Parameters<GetCommentsRequest>,
    ) -> Result<CallToolResult, String> {
        let (resolved, reference) = self.resolve(&request.link).await?;
        let rect = match request.rect {
            Some(rect) => Some(rect.validate()?),
            None => reference.rect,
        };
        let comments = resolved
            .design
            .comments
            .iter()
            .filter(|comment| {
                request
                    .comment_id
                    .as_deref()
                    .is_none_or(|comment_id| comment.id == comment_id)
            })
            .filter(|comment| {
                rect.as_ref().is_none_or(|rect| {
                    comment_design_position(&resolved.design, comment)
                        .is_some_and(|(x, y)| rect.contains(x, y))
                })
            })
            .map(|comment| comment_summary(&resolved.capture, &resolved.design, comment))
            .collect::<Vec<_>>();
        if let Some(comment_id) = request.comment_id.as_deref() {
            if comments.is_empty() {
                return Err(format!("comment not found: {comment_id}"));
            }
        }
        Ok(CallToolResult::structured(json!({
            "design": design_summary(&resolved.capture, &resolved.design),
            "query": { "commentId": request.comment_id, "rect": rect },
            "comments": comments,
        })))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DesignBridgeMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("designbridge", env!("CARGO_PKG_VERSION"))
                    .with_title("DesignBridge")
                    .with_description("Read locally captured Lanhu design context and assets"),
            )
            .with_instructions(
                "DesignBridge exposes read-only context from locally captured Lanhu designs. Resolve a supplied designbridge:// link first, then use focused node-id or rect context queries. Use get_design_screenshot for visual reference, get_assets only for the slice layers needed by the implementation, and get_comments for design review context. Comment position.x/y values use the design frame coordinate space and each comment includes candidate UI layers at that point.",
            )
    }
}

fn captures_root() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("DESIGNBRIDGE_CAPTURES_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::data_dir()
        .map(|path| path.join("com.designbridge.app").join("captures"))
        .ok_or_else(|| {
            "cannot determine the DesignBridge data directory; set DESIGNBRIDGE_CAPTURES_DIR"
                .to_string()
        })
}

fn design_link(project_id: &str, image_id: &str) -> String {
    let mut url = Url::parse("designbridge://design/").expect("valid design link base");
    url.path_segments_mut()
        .expect("design link supports path segments")
        .push(project_id)
        .push(image_id);
    url.to_string()
}

fn combined_query(url: &Url) -> HashMap<String, String> {
    let mut query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if let Some(fragment) = url.fragment() {
        if let Some((_, fragment_query)) = fragment.split_once('?') {
            for (key, value) in url::form_urlencoded::parse(fragment_query.as_bytes()) {
                query
                    .entry(key.into_owned())
                    .or_insert_with(|| value.into_owned());
            }
        }
    }
    query
}

fn parse_rect(value: &str) -> Result<LayerFrame, String> {
    let values: Vec<f64> = value
        .split(',')
        .map(|part| part.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| "rect must use x,y,width,height numeric values".to_string())?;
    if values.len() != 4 {
        return Err("rect must use x,y,width,height".to_string());
    }
    RectInput {
        x: values[0],
        y: values[1],
        width: values[2],
        height: values[3],
    }
    .validate()
}

fn parse_design_reference(link: &str) -> Result<DesignReference, String> {
    let url = Url::parse(link.trim()).map_err(|_| "link must be a complete URL".to_string())?;
    let query = combined_query(&url);
    let (project_id, image_id) = if url.scheme() == "designbridge"
        && url.host_str() == Some("design")
    {
        let segments: Vec<&str> = url.path_segments().into_iter().flatten().collect();
        if segments.len() < 2 {
            return Err("DesignBridge link must contain project id and image id".to_string());
        }
        (segments[0].to_string(), segments[1].to_string())
    } else if matches!(
        url.host_str(),
        Some("lanhuapp.com") | Some("www.lanhuapp.com") | Some("lanhu.com") | Some("www.lanhu.com")
    ) {
        let project_id = query
            .get("project_id")
            .or_else(|| query.get("pid"))
            .cloned()
            .ok_or_else(|| "Lanhu link is missing project_id/pid".to_string())?;
        let image_id = query
            .get("image_id")
            .cloned()
            .ok_or_else(|| "Lanhu link is missing image_id".to_string())?;
        (project_id, image_id)
    } else {
        return Err(
            "only designbridge:// design links and Lanhu design URLs are supported".to_string(),
        );
    };
    let node_id = query
        .get("node-id")
        .or_else(|| query.get("node_id"))
        .cloned();
    let rect = query
        .get("rect")
        .map(|value| parse_rect(value))
        .transpose()?;
    Ok(DesignReference {
        project_id,
        image_id,
        node_id,
        rect,
    })
}

fn design_frame(design: &CapturedDesign) -> Option<Value> {
    if let Some(frame) = &design.coordinate_space {
        return Some(json!({
            "width": frame.width,
            "height": frame.height,
            "unit": frame.unit,
            "platform": frame.platform,
        }));
    }
    match (design.width, design.height) {
        (Some(width), Some(height)) => {
            Some(json!({ "width": width, "height": height, "unit": "px" }))
        }
        _ => None,
    }
}

fn design_summary(capture: &CaptureResult, design: &CapturedDesign) -> Value {
    json!({
        "link": design_link(&capture.project_id, &design.id),
        "projectId": capture.project_id,
        "imageId": design.id,
        "projectName": capture.project_name,
        "designName": design.name,
        "dataVersion": capture.data_version,
        "capturedAt": capture.captured_at,
        "frame": design_frame(design),
        "imagePath": design.local_path,
        "layerCount": capture.layers.len(),
        "sliceCount": capture.slices.len(),
        "commentCount": design.comments.len(),
        "slicesComplete": capture.slices_complete,
    })
}

fn design_coordinate_dimensions(design: &CapturedDesign) -> Option<(f64, f64)> {
    let dimensions = design
        .coordinate_space
        .as_ref()
        .map(|frame| (frame.width, frame.height))
        .or_else(|| design.width.zip(design.height))?;
    (dimensions.0.is_finite()
        && dimensions.1.is_finite()
        && dimensions.0 > 0.0
        && dimensions.1 > 0.0)
        .then_some(dimensions)
}

fn comment_design_position(design: &CapturedDesign, comment: &DesignComment) -> Option<(f64, f64)> {
    let (design_width, design_height) = design_coordinate_dimensions(design)?;
    let source_width = comment.source_width.or(design.width)?;
    let source_height = comment.source_height.or(design.height)?;
    let x = comment.x?;
    let y = comment.y?;
    if !source_width.is_finite()
        || !source_height.is_finite()
        || source_width <= 0.0
        || source_height <= 0.0
        || !x.is_finite()
        || !y.is_finite()
    {
        return None;
    }
    Some((
        (x / source_width) * design_width,
        (y / source_height) * design_height,
    ))
}

fn comment_layer_candidates(
    capture: &CaptureResult,
    comment: &DesignComment,
    position: Option<(f64, f64)>,
) -> Vec<Value> {
    let Some((x, y)) = position else {
        return Vec::new();
    };
    let mut layers = capture
        .layers
        .iter()
        .filter(|layer| {
            layer.visible
                && layer
                    .frame
                    .as_ref()
                    .is_some_and(|frame| frame.contains(x, y))
        })
        .collect::<Vec<_>>();
    layers.sort_by(|left, right| {
        let left_is_target = comment.target_id.as_deref() == Some(left.id.as_str());
        let right_is_target = comment.target_id.as_deref() == Some(right.id.as_str());
        right_is_target
            .cmp(&left_is_target)
            .then_with(|| right.has_slice.cmp(&left.has_slice))
            .then_with(|| {
                frame_area(left)
                    .partial_cmp(&frame_area(right))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| right.depth.cmp(&left.depth))
            .then_with(|| right.order.cmp(&left.order))
    });
    layers.truncate(8);
    layers.into_iter().map(layer_summary).collect()
}

fn comment_summary(
    capture: &CaptureResult,
    design: &CapturedDesign,
    comment: &DesignComment,
) -> Value {
    let position = comment_design_position(design, comment);
    let dimensions = design_coordinate_dimensions(design);
    let unit = design
        .coordinate_space
        .as_ref()
        .map(|frame| frame.unit.as_str())
        .unwrap_or("px");
    let position_value = position.and_then(|(x, y)| {
        let (width, height) = dimensions?;
        Some(json!({
            "x": x,
            "y": y,
            "unit": unit,
            "normalizedX": x / width,
            "normalizedY": y / height,
            "designWidth": width,
            "designHeight": height,
        }))
    });
    let target = comment.target_id.as_ref().map(|target_id| {
        json!({
            "id": target_id,
            "type": comment.target_type,
        })
    });
    let target_layer = comment.target_id.as_deref().and_then(|target_id| {
        capture
            .layers
            .iter()
            .find(|layer| layer.id == target_id)
            .map(layer_summary)
    });
    let layer_candidates = comment_layer_candidates(capture, comment, position);
    json!({
        "id": comment.id,
        "index": comment.index,
        "author": comment.author,
        "content": comment.content,
        "createdAt": comment.created_at,
        "resolved": comment.resolved,
        "version": {
            "id": comment.version_id,
            "name": comment.version_name,
        },
        "position": position_value,
        "target": target,
        "targetLayer": target_layer,
        "layersAtPosition": layer_candidates,
        "replies": comment.replies,
    })
}

fn layer_summary(layer: &InspectableLayer) -> Value {
    json!({
        "id": layer.id,
        "parentId": layer.parent_id,
        "name": layer.name,
        "layerType": layer.layer_type,
        "depth": layer.depth,
        "frame": layer.frame,
        "visible": layer.visible,
        "isAsset": layer.is_asset,
        "hasSlice": layer.has_slice,
        "text": layer.text,
    })
}

fn asset_summary(slice: &CapturedSlice) -> Value {
    json!({
        "id": slice.id,
        "name": slice.name,
        "width": slice.width,
        "height": slice.height,
        "format": slice.output_format,
        "scale": slice.output_scale,
        "directory": slice.output_dir,
        "path": slice.local_path,
        "error": slice.error,
    })
}

fn is_descendant_of(
    layer: &InspectableLayer,
    ancestor_id: &str,
    parent_map: &HashMap<&str, Option<&str>>,
) -> bool {
    let mut parent = layer.parent_id.as_deref();
    let mut visited = HashSet::new();
    while let Some(parent_id) = parent {
        if parent_id == ancestor_id {
            return true;
        }
        if !visited.insert(parent_id) {
            break;
        }
        parent = parent_map.get(parent_id).copied().flatten();
    }
    false
}

fn frame_area(layer: &InspectableLayer) -> f64 {
    layer
        .frame
        .as_ref()
        .map(|frame| frame.width * frame.height)
        .unwrap_or(f64::MAX)
}

fn image_mime(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => "image/png",
    }
}

async fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("cannot inspect image {}: {error}", path.display()))?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "image exceeds the 50 MB MCP limit: {}",
            path.display()
        ));
    }
    tokio::fs::read(path)
        .await
        .map_err(|error| format!("cannot read image {}: {error}", path.display()))
}

async fn read_design_image(
    path: &Path,
    design: &CapturedDesign,
    crop: Option<&LayerFrame>,
) -> Result<(Vec<u8>, &'static str, u32, u32), String> {
    let bytes = read_limited(path).await?;
    let image = image::load_from_memory(&bytes)
        .map_err(|error| format!("cannot decode design image {}: {error}", path.display()))?;
    let (pixel_width, pixel_height) = image.dimensions();
    let Some(crop) = crop else {
        return Ok((
            bytes,
            image_mime(path.to_string_lossy().as_ref()),
            pixel_width,
            pixel_height,
        ));
    };
    let coordinate_width = design
        .coordinate_space
        .as_ref()
        .map(|frame| frame.width)
        .or(design.width)
        .ok_or_else(|| {
            "design width is unavailable, so the image cannot be cropped by coordinates".to_string()
        })?;
    let coordinate_height = design
        .coordinate_space
        .as_ref()
        .map(|frame| frame.height)
        .or(design.height)
        .ok_or_else(|| {
            "design height is unavailable, so the image cannot be cropped by coordinates"
                .to_string()
        })?;
    if !coordinate_width.is_finite()
        || !coordinate_height.is_finite()
        || coordinate_width <= 0.0
        || coordinate_height <= 0.0
    {
        return Err("design coordinate space must have positive width and height".to_string());
    }
    let left = crop.x.max(0.0).min(coordinate_width);
    let top = crop.y.max(0.0).min(coordinate_height);
    let right_coordinate = (crop.x + crop.width).max(0.0).min(coordinate_width);
    let bottom_coordinate = (crop.y + crop.height).max(0.0).min(coordinate_height);
    let x = ((left / coordinate_width) * pixel_width as f64).floor() as u32;
    let y = ((top / coordinate_height) * pixel_height as f64).floor() as u32;
    let right = ((right_coordinate / coordinate_width) * pixel_width as f64).ceil() as u32;
    let bottom = ((bottom_coordinate / coordinate_height) * pixel_height as f64).ceil() as u32;
    if right <= x || bottom <= y {
        return Err("crop rectangle is outside the design frame".to_string());
    }
    let cropped = image.crop_imm(x, y, right - x, bottom - y);
    let mut encoded = Cursor::new(Vec::new());
    cropped
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("cannot encode cropped design image: {error}"))?;
    Ok((
        encoded.into_inner(),
        "image/png",
        cropped.width(),
        cropped.height(),
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = DesignBridgeMcp::new(captures_root()?);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_designbridge_link_with_selection() {
        let reference = parse_design_reference(
            "designbridge://design/project-1/image-2?node-id=layer%3A3&rect=1,2,30,40",
        )
        .unwrap();
        assert_eq!(reference.project_id, "project-1");
        assert_eq!(reference.image_id, "image-2");
        assert_eq!(reference.node_id.as_deref(), Some("layer:3"));
        assert_eq!(reference.rect.unwrap().width, 30.0);
    }

    #[test]
    fn keeps_different_layer_links_distinct() {
        let first = parse_design_reference(
            "designbridge://design/project-1/image-2?node-id=layer%3Afirst",
        )
        .unwrap();
        let second = parse_design_reference(
            "designbridge://design/project-1/image-2?node-id=layer%3Asecond",
        )
        .unwrap();

        assert_ne!(first.node_id, second.node_id);
        assert_eq!(first.project_id, second.project_id);
        assert_eq!(first.image_id, second.image_id);
    }

    #[test]
    fn parses_lanhu_hash_query() {
        let reference = parse_design_reference(
            "https://lanhuapp.com/web/#/item/project/detailDetach?pid=p1&project_id=p1&image_id=i1",
        )
        .unwrap();
        assert_eq!(reference.project_id, "p1");
        assert_eq!(reference.image_id, "i1");
    }

    #[test]
    fn creates_stable_design_link() {
        assert_eq!(
            design_link("project id", "image/id"),
            "designbridge://design/project%20id/image%2Fid"
        );
    }

    #[test]
    fn converts_comment_position_to_the_design_coordinate_space() {
        let design = CapturedDesign {
            id: "design-1".to_string(),
            name: "Screen".to_string(),
            width: Some(187.5),
            height: Some(693.0),
            coordinate_space: Some(DesignCoordinateSpace {
                platform: "android".to_string(),
                width: 375.0,
                height: 1386.0,
                unit: "dp".to_string(),
            }),
            update_time: None,
            has_comment: true,
            comments: Vec::new(),
            remote_url: String::new(),
            local_path: None,
            error: None,
        };
        let comment = DesignComment {
            id: "comment-1".to_string(),
            index: 1,
            author: "Reviewer".to_string(),
            content: "Use the night colors".to_string(),
            created_at: None,
            resolved: false,
            x: Some(44.598941496986754),
            y: Some(417.70549927641105),
            source_width: Some(187.5),
            source_height: Some(693.0),
            version_id: Some("version-10".to_string()),
            version_name: Some("版本10".to_string()),
            target_id: None,
            target_type: None,
            replies: Vec::new(),
        };

        let (x, y) = comment_design_position(&design, &comment).expect("valid position");
        assert!((x - 89.19788299397351).abs() < 0.000_001);
        assert!((y - 835.4109985528221).abs() < 0.000_001);
    }
}
