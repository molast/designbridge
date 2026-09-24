    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn accepts_lanhu_project_and_invite_urls() {
        assert!(lanhu_url("https://lanhuapp.com/link/#/invite?sid=abc").is_ok());
        assert!(lanhu_url("https://lanhuapp.com/web/#/item/project/stage?tid=t1&pid=p1").is_ok());
    }

    #[test]
    fn extracts_route_from_hash_query() {
        let url = lanhu_url("https://lanhuapp.com/web/#/item/project/detailDetach?tid=t1&pid=p1&project_id=p1&image_id=i1&child=c1").unwrap();
        let route = lanhu_route(&url);
        assert_eq!(route.team_id.as_deref(), Some("t1"));
        assert_eq!(route.project_id.as_deref(), Some("p1"));
        assert_eq!(route.image_id.as_deref(), Some("i1"));
        assert_eq!(route.child.as_deref(), Some("c1"));
    }

    #[test]
    fn rejects_non_lanhu_and_insecure_urls() {
        assert!(lanhu_url("https://example.com/design").is_err());
        assert!(lanhu_url("http://lanhuapp.com/web/").is_err());
    }

    #[test]
    fn validates_asset_hosts() {
        assert!(safe_asset_url("https://alipic.lanhuapp.com/example.png").is_ok());
        assert!(safe_asset_url("https://bucket.oss-cn-hangzhou.aliyuncs.com/a.png").is_ok());
        assert!(safe_asset_url("https://127.0.0.1/a.png").is_err());
        assert!(safe_asset_url("https://example.com/a.png").is_err());
    }

    #[test]
    fn removes_only_oss_preview_processing() {
        let url = original_asset_url(
            "https://alipic.lanhuapp.com/a.png?token=abc&x-oss-process=image%2Fresize,w_300",
        )
        .expect("valid asset URL");
        assert_eq!(url.as_str(), "https://alipic.lanhuapp.com/a.png?token=abc");
    }

    #[test]
    fn parses_title_protocol() {
        let title = "__DESIGNBRIDGE__|123|payload|0|1|YWJj";
        let message = parse_title_message(title).expect("valid title message");
        assert_eq!(message.capture_id, "123");
        assert_eq!(message.kind, "payload");
        assert_eq!(message.index, 0);
        assert_eq!(message.total, 1);
        assert_eq!(message.data, "YWJj");
    }

    #[test]
    fn sanitizes_cross_platform_file_names() {
        assert_eq!(sanitize_filename("登录/注册:页面?"), "登录_注册_页面_");
        assert_eq!(sanitize_filename("..."), "untitled");
    }

    #[test]
    fn validates_capture_ids_before_deletion() {
        assert!(valid_capture_id("1788429897760-1"));
        assert!(!valid_capture_id("../capture"));
        assert!(!valid_capture_id("capture/child"));
        assert!(!valid_capture_id(""));
    }

    #[test]
    fn keeps_all_detected_slices() {
        let design_json = serde_json::json!({
            "info": {
                "layers": [
                    {
                        "web_id": "background",
                        "name": "background",
                        "isAsset": true,
                        "images": {"png_xxxhd": "https://alipic.lanhuapp.com/background.png"},
                        "width": 400,
                        "height": 200
                    },
                    {
                        "web_id": "icon-tab",
                        "name": "icon/inside/tab_rat",
                        "isAsset": true,
                        "images": {"png_xxxhd": "https://alipic.lanhuapp.com/icon.png"},
                        "width": 24,
                        "height": 24
                    }
                ]
            }
        });
        let slices = collect_slices(&design_json);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].name, "background");
        assert_eq!(slices[1].name, "icon/inside/tab_rat");
        assert_eq!(slices[1].url, "https://alipic.lanhuapp.com/icon.png");
    }

    #[test]
    fn keeps_same_named_slices_when_their_layer_ids_differ() {
        let design_json = serde_json::json!({
            "layers": [
                {
                    "id": "slice-a",
                    "name": "icon/general/enter",
                    "isAsset": true,
                    "images": {"png_xxxhd": "https://alipic.lanhuapp.com/enter.png"},
                    "width": 12,
                    "height": 12
                },
                {
                    "id": "slice-b",
                    "name": "icon/general/enter",
                    "isAsset": true,
                    "images": {"png_xxxhd": "https://alipic.lanhuapp.com/enter.png"},
                    "width": 12,
                    "height": 12
                }
            ]
        });

        let slices = collect_slices(&design_json);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].id, "slice-a");
        assert_eq!(slices[1].id, "slice-b");
    }

    #[test]
    fn reads_android_coordinate_space_from_artboard_frame() {
        let design_json = serde_json::json!({
            "artboard": {
                "frame": {"left": 54561, "top": 48540, "width": 375, "height": 2337}
            }
        });
        assert_eq!(
            android_coordinate_space(&design_json),
            Some(DesignCoordinateSpace {
                platform: "android".to_string(),
                width: 375.0,
                height: 2337.0,
                unit: "dp".to_string(),
            })
        );
    }

    #[test]
    fn extracts_comments_from_the_latest_design_version() {
        let design = serde_json::json!({
            "id": "design-1",
            "name": "Commented design",
            "width": 187.5,
            "height": 400,
            "url": "https://alipic.lanhuapp.com/design.png",
            "latest_version": "version-2",
            "versions": [
                {
                    "id": "version-1",
                    "version_info": "版本1",
                    "width": 187.5,
                    "height": 400,
                    "comments": [{"id": "old", "content": "旧评论"}]
                },
                {
                    "id": "version-2",
                    "version_info": "版本2",
                    "width": 187.5,
                    "height": 400,
                    "comments": [{
                        "id": "comment-1",
                        "content": "夜间#042A36-#440A0B",
                        "create_time": 1787739576,
                        "position_x": 0.25,
                        "position_y": 0.5,
                        "text": "7",
                        "user": {"nickname": "郑向萍"},
                        "replies": [{
                            "id": "reply-1",
                            "content": "已确认",
                            "user": {"name": "Reviewer"}
                        }]
                    }]
                }
            ]
        });

        let captured = design_from_value(&design, "fallback").expect("valid design");
        assert_eq!(captured.comments.len(), 1);
        let comment = &captured.comments[0];
        assert_eq!(comment.id, "comment-1");
        assert_eq!(comment.index, 7);
        assert_eq!(comment.author, "郑向萍");
        assert_eq!(comment.content, "夜间#042A36-#440A0B");
        assert_eq!(comment.created_at.as_deref(), Some("1787739576"));
        assert_eq!(comment.x, Some(46.875));
        assert_eq!(comment.y, Some(200.0));
        assert_eq!(comment.version_id.as_deref(), Some("version-2"));
        assert_eq!(comment.version_name.as_deref(), Some("版本2"));
        assert_eq!(comment.replies[0].content, "已确认");
    }

    #[test]
    fn prefers_lanhu_visual_frame_for_rotated_layers() {
        let layer = serde_json::json!({
            "rotation": 180,
            "frame": {"left": 273, "top": 1021, "width": 12, "height": 12},
            "realFrame": {"left": 261, "top": 1021, "width": 12, "height": 12}
        });

        assert_eq!(
            layer_frame(&layer),
            Some(LayerFrame {
                x: 261.0,
                y: 1021.0,
                width: 12.0,
                height: 12.0,
            })
        );
    }

    #[test]
    fn normalizes_rotated_frames_from_legacy_captures_once() {
        let mut layer = InspectableLayer {
            id: "legacy-flipped-icon".to_string(),
            parent_id: None,
            name: "icon/general/enter".to_string(),
            layer_type: "bitmapLayer".to_string(),
            depth: 1,
            order: 0,
            frame: Some(LayerFrame {
                x: 273.0,
                y: 1021.0,
                width: 12.0,
                height: 12.0,
            }),
            frame_is_visual: false,
            opacity: 1.0,
            rotation: 180.0,
            visible: true,
            pass_through: false,
            radius: LayerRadius::default(),
            fills: Vec::new(),
            borders: Vec::new(),
            shadows: Vec::new(),
            blurs: Vec::new(),
            text: None,
            is_asset: true,
            has_slice: true,
        };

        normalize_legacy_layer_frames(std::slice::from_mut(&mut layer));
        assert_eq!(layer.frame.as_ref().unwrap().x, 261.0);
        assert_eq!(layer.frame.as_ref().unwrap().y, 1021.0);
        assert!(layer.frame_is_visual);

        normalize_legacy_layer_frames(std::slice::from_mut(&mut layer));
        assert_eq!(layer.frame.as_ref().unwrap().x, 261.0);
    }

    #[test]
    fn extracts_inspectable_layers_and_prefers_path_radius() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "name": "Screen",
                "type": "artboard",
                "frame": {"left": 54561, "top": 48540, "width": 375, "height": 2337},
                "layers": [{
                    "id": "5046:66772",
                    "name": "Frame 427318893",
                    "type": "artboard",
                    "frame": {"left": 221, "top": 138, "width": 80, "height": 28},
                    "opacity": 1,
                    "visible": true,
                    "radius": {"topLeft": 0, "topRight": 0, "bottomRight": 0, "bottomLeft": 0},
                    "paths": [{
                        "radius": {"topLeft": 20, "topRight": 20, "bottomRight": 20, "bottomLeft": 20}
                    }],
                    "style": {
                        "fills": [{
                            "type": "color",
                            "isEnabled": true,
                            "opacity": 1,
                            "boundVariables": {"color": {"name": "sys/bg/bg-1"}},
                            "color": {"value": "rgba(245,245,245,1)"}
                        }],
                        "borders": [],
                        "shadows": [],
                        "blurs": []
                    },
                    "layers": []
                }]
            }
        });

        let layers = collect_layers(&design_json);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].frame.as_ref().unwrap().x, 0.0);
        assert_eq!(layers[0].frame.as_ref().unwrap().y, 0.0);
        assert_eq!(layers[1].parent_id.as_deref(), Some("root"));
        assert_eq!(layers[1].frame.as_ref().unwrap().x, 221.0);
        assert_eq!(layers[1].frame.as_ref().unwrap().y, 138.0);
        assert!(!layers[1].frame_is_visual);
        assert_eq!(layers[1].radius.top_left, Some(20.0));
        assert_eq!(layers[1].fills[0].token.as_deref(), Some("sys/bg/bg-1"));
        assert_eq!(
            layers[1].fills[0].color.as_deref(),
            Some("rgba(245,245,245,1)")
        );
    }

    #[test]
    fn keeps_only_radius_fields_present_in_json() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "frame": {"left": 0, "top": 0, "width": 100, "height": 100},
                "layers": [{
                    "id": "layer",
                    "frame": {"left": 0, "top": 0, "width": 100, "height": 100},
                    "radius": {"topLeft": 0},
                    "layers": []
                }]
            }
        });

        let layers = collect_layers(&design_json);
        assert_eq!(layers[1].radius.top_left, Some(0.0));
        assert_eq!(layers[1].radius.top_right, None);
        assert_eq!(layers[1].radius.bottom_right, None);
        assert_eq!(layers[1].radius.bottom_left, None);
    }

    #[test]
    fn extracts_every_rich_text_style_run() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "frame": {"left": 0, "top": 0, "width": 375, "height": 800},
                "layers": [{
                    "id": "mixed-text",
                    "name": "Goalkeeper, #22",
                    "type": "textLayer",
                    "frame": {"left": 82, "top": 150, "width": 90, "height": 14},
                    "text": {
                        "value": "Goalkeeper, #22",
                        "style": {
                            "content": "Goalkeeper, #22",
                            "font": {"name": "Sofascore Sans", "size": 12, "fontWeight": 400},
                            "color": {"value": "rgba(153,153,153,1)"}
                        },
                        "styles": [
                            {
                                "from": 0,
                                "to": 12,
                                "content": "Goalkeeper, ",
                                "font": {
                                    "name": "Sofascore Sans",
                                    "postScriptName": "Sofascore Sans-Regular",
                                    "type": "Regular",
                                    "size": 12,
                                    "fontWeight": 400,
                                    "align": "left",
                                    "verticalAlignment": "center",
                                    "letterSpacing": {"unit": "percent", "value": 0},
                                    "lineHeight": {"unit": "AUTO"}
                                },
                                "color": {"value": "rgba(153,153,153,1)"}
                            },
                            {
                                "from": 12,
                                "to": 15,
                                "content": "#22",
                                "font": {
                                    "name": "Sofascore Sans",
                                    "postScriptName": "Sofascore Sans-Regular",
                                    "type": "Regular",
                                    "size": 12,
                                    "fontWeight": 400,
                                    "align": "left",
                                    "verticalAlignment": "center",
                                    "letterSpacing": {"unit": "percent", "value": 0},
                                    "lineHeight": {"unit": "AUTO"}
                                },
                                "color": {"value": "rgba(208,164,5,1)"}
                            }
                        ]
                    },
                    "layers": []
                }]
            }
        });

        let layers = collect_layers(&design_json);
        let text = layers[1].text.as_ref().expect("text layer");
        assert_eq!(text.styles.len(), 2);
        assert_eq!(text.styles[0].content, "Goalkeeper, ");
        assert_eq!(
            text.styles[0].letter_spacing_unit.as_deref(),
            Some("percent")
        );
        assert_eq!(text.styles[0].line_height_unit.as_deref(), Some("AUTO"));
        assert_eq!(text.styles[1].content, "#22");
        assert_eq!(text.styles[1].color.as_deref(), Some("rgba(208,164,5,1)"));
    }

    #[test]
    fn links_downloadable_slice_to_its_layer_id() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "frame": {"left": 0, "top": 0, "width": 375, "height": 800},
                "layers": [{
                    "id": "asset-layer",
                    "name": "icon/inside/tab-stats-red",
                    "type": "bitmapLayer",
                    "frame": {"left": 242, "top": 102, "width": 12, "height": 12},
                    "image": {"imageUrl": "https://alipic.lanhuapp.com/icon.png"},
                    "layers": []
                }]
            }
        });
        let slices = collect_slices(&design_json);
        let mut layers = collect_layers(&design_json);
        link_slices_to_layers(&mut layers, &slices);

        assert_eq!(slices.len(), 1);
        assert!(layers[1].has_slice);
        assert!(layers[1].is_asset);
    }

    #[test]
    fn skips_one_pixel_and_fully_transparent_slices() {
        let one_pixel = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])));
        assert!(should_skip_slice(&one_pixel));

        let transparent =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(20, 12, Rgba([255, 255, 255, 0])));
        assert!(should_skip_slice(&transparent));

        let visible =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(20, 12, Rgba([255, 255, 255, 255])));
        assert!(!should_skip_slice(&visible));
    }

    #[test]
    fn counts_only_actual_design_download_errors() {
        fn design(local_path: Option<&str>, error: Option<&str>) -> CapturedDesign {
            CapturedDesign {
                id: "design".to_string(),
                name: "Design".to_string(),
                width: Some(100.0),
                height: Some(100.0),
                coordinate_space: None,
                update_time: None,
                has_comment: false,
                comments: Vec::new(),
                remote_url: "https://example.com/design.png".to_string(),
                local_path: local_path.map(str::to_string),
                error: error.map(str::to_string),
                layers: Vec::new(),
                slices: Vec::new(),
                slice_downloaded_count: 0,
                slice_failed_count: 0,
                slice_total_count: 0,
                slices_complete: true,
            }
        }

        let designs = vec![
            design(Some("/tmp/design.png"), None),
            design(None, None), // metadata-only sibling page, loaded on demand
            design(None, Some("download failed")),
        ];

        assert_eq!(failed_design_count(&designs), 1);
    }

    #[test]
    fn limits_slice_exports_to_supported_platform_scales() {
        assert_eq!(
            export_target("android", "xxhdpi"),
            Some(ExportTarget {
                label: "mipmap-xxhdpi",
                directory: "mipmap-xxhdpi",
                suffix: "",
                factor: 3.0,
            })
        );
        assert_eq!(export_target("ios", "3x").unwrap().suffix, "@3x");
        assert!(export_target("web", "1x").is_none());
        assert!(export_target("android", "5x").is_none());
    }

    #[test]
    fn calculates_slice_export_pixel_dimensions() {
        assert_eq!(export_dimension(20.0, 1.0).unwrap(), 20);
        assert_eq!(export_dimension(20.0, 1.5).unwrap(), 30);
        assert_eq!(export_dimension(20.0, 4.0).unwrap(), 80);
        assert!(export_dimension(0.0, 3.0).is_err());
    }

    #[test]
    fn limits_slice_export_encoders_and_flattens_jpg_alpha() {
        let transparent =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([20, 40, 60, 0])));
        assert!(encode_export_image(&transparent, "png").is_ok());
        assert!(encode_export_image(&transparent, "webp").is_ok());
        let jpg = encode_export_image(&transparent, "jpg").unwrap();
        let pixel = image::load_from_memory(&jpg)
            .unwrap()
            .to_rgb8()
            .get_pixel(0, 0)
            .0;
        assert!(pixel.iter().all(|channel| *channel > 245));
        assert!(encode_export_image(&transparent, "avif").is_err());
    }

    #[test]
    fn normalizes_lanhu_bare_json_url() {
        let value = serde_json::json!({
            "version": {"json_url": "alipic.lanhuapp.com/design.json"}
        });
        assert_eq!(
            find_json_url(&value).as_deref(),
            Some("https://alipic.lanhuapp.com/design.json")
        );
    }

    #[test]
    fn limits_browser_extension_manager_targets() {
        assert_eq!(
            browser_extension_manager_target("chrome").unwrap(),
            ("Google Chrome", "chrome://extensions/")
        );
        assert_eq!(
            browser_extension_manager_target("edge").unwrap(),
            ("Microsoft Edge", "edge://extensions/")
        );
        assert!(browser_extension_manager_target("safari").is_err());
    }

    #[test]
    fn adds_browser_capture_marker_without_changing_lanhu_route() {
        let source = lanhu_url(
            "https://lanhuapp.com/web/#/item/project/detailDetach?pid=project&image_id=design",
        )
        .unwrap();
        let marked = browser_capture_url(source, "123-4");

        assert_eq!(
            marked
                .query_pairs()
                .find(|(key, _)| key == "designbridge_capture")
                .map(|(_, value)| value.into_owned())
                .as_deref(),
            Some("123-4")
        );
        let route = lanhu_route(&marked);
        assert_eq!(route.project_id.as_deref(), Some("project"));
        assert_eq!(route.image_id.as_deref(), Some("design"));
    }

    #[test]
    fn treats_bare_image_links_as_single_pages_but_sets_as_collections() {
        let page = lanhu_url(
            "https://lanhuapp.com/web/#/item/project/detailDetach?pid=project&image_id=design",
        )
        .unwrap();
        assert!(lanhu_route(&page).single_page);

        let set = lanhu_url(
            "https://lanhuapp.com/web/#/item/project/detailDetach?pid=project&image_id=design&type=set",
        )
        .unwrap();
        assert!(!lanhu_route(&set).single_page);

        let section = lanhu_url(
            "https://lanhuapp.com/web/#/item/project/detailDetach?pid=project&image_id=design&type=sectionImageChange",
        )
        .unwrap();
        assert!(!lanhu_route(&section).single_page);
    }

    #[test]
    fn reads_optional_browser_capture_id_from_camel_case() {
        let request: BrowserCaptureRequest = serde_json::from_value(serde_json::json!({
            "version": 1,
            "type": "capture",
            "captureId": "123-4",
            "url": "https://lanhuapp.com/web/#/?pid=project&image_id=design",
            "cookie": "",
            "authToken": "secret"
        }))
        .unwrap();

        assert_eq!(request.capture_id.as_deref(), Some("123-4"));
        assert_eq!(request.auth_token, "secret");
    }

    #[test]
    fn creates_lanhu_basic_authorization_from_page_token() {
        let value = lanhu_authorization_header("secret").unwrap();
        assert_eq!(value.to_str().unwrap(), "Basic c2VjcmV0Og==");
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn distinguishes_installed_extension_from_loaded_auto_capture_script() {
        let old_settings = serde_json::json!({
            "disable_reasons": [],
            "active_permissions": { "scriptable_host": [] }
        });
        assert_eq!(extension_setting_state(&old_settings), (true, false));

        let current_settings = serde_json::json!({
            "disable_reasons": [],
            "active_permissions": {
                "scriptable_host": ["https://*.lanhuapp.com/*"]
            }
        });
        assert_eq!(extension_setting_state(&current_settings), (true, true));
    }

    #[test]
    fn finds_figma_bitmap_layer_slices() {
        let design_json = serde_json::json!({
            "artboard": {
                "layers": [{
                    "type": "bitmapLayer",
                    "id": "I1",
                    "name": "icon/inside/tab-overview-gary",
                    "image": {
                        "imageUrl": "https://lanhu-oss-2537-2.lanhuapp.com/FigmaSlicePNG044970105cf846b14bc6ed13f63137b7.png",
                        "svgUrl": "https://lanhu-oss-2537-2.lanhuapp.com/FigmaSliceSVG6615709efefa7da031590121756a086a.svg"
                    },
                    "frame": {"width": 12, "height": 12}
                }]
            },
            "assets": []
        });
        let slices = collect_slices(&design_json);
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].name, "icon/inside/tab-overview-gary");
        assert_eq!(slices[0].url, "https://lanhu-oss-2537-2.lanhuapp.com/FigmaSlicePNG044970105cf846b14bc6ed13f63137b7.png");
    }
