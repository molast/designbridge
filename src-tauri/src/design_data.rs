use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesignCoordinateSpace {
    pub platform: String,
    pub width: f64,
    pub height: f64,
    pub unit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesignCommentReply {
    pub id: String,
    pub author: String,
    pub content: String,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesignComment {
    pub id: String,
    pub index: usize,
    pub author: String,
    pub content: String,
    pub created_at: Option<String>,
    pub resolved: bool,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub source_width: Option<f64>,
    pub source_height: Option<f64>,
    pub version_id: Option<String>,
    pub version_name: Option<String>,
    pub target_id: Option<String>,
    pub target_type: Option<String>,
    #[serde(default)]
    pub replies: Vec<DesignCommentReply>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedDesign {
    pub id: String,
    pub name: String,
    pub width: Option<f64>,
    pub height: Option<f64>,
    #[serde(default)]
    pub coordinate_space: Option<DesignCoordinateSpace>,
    pub update_time: Option<String>,
    pub has_comment: bool,
    #[serde(default)]
    pub comments: Vec<DesignComment>,
    pub remote_url: String,
    pub local_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedSlice {
    pub id: String,
    pub name: String,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub remote_url: String,
    pub output_format: String,
    pub output_scale: f64,
    pub output_dir: String,
    pub local_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LayerFrame {
    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerRadius {
    pub top_left: f64,
    pub top_right: f64,
    pub bottom_right: f64,
    pub bottom_left: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerPaint {
    pub paint_type: String,
    pub color: Option<String>,
    pub token: Option<String>,
    pub opacity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerBorder {
    pub width: f64,
    pub style: String,
    pub color: Option<String>,
    pub token: Option<String>,
    pub opacity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerShadow {
    pub shadow_type: String,
    pub color: Option<String>,
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
    pub spread: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerBlur {
    pub blur_type: String,
    pub radius: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerTextStyle {
    pub content: String,
    pub from: Option<usize>,
    pub to: Option<usize>,
    pub font_family: Option<String>,
    pub post_script_name: Option<String>,
    pub font_style: Option<String>,
    pub font_size: Option<f64>,
    pub font_weight: Option<f64>,
    pub alignment: Option<String>,
    pub vertical_alignment: Option<String>,
    pub line_height: Option<f64>,
    pub line_height_unit: Option<String>,
    pub letter_spacing: Option<f64>,
    pub letter_spacing_unit: Option<String>,
    pub color: Option<String>,
    pub token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerText {
    pub content: String,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_weight: Option<f64>,
    pub alignment: Option<String>,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<f64>,
    pub color: Option<String>,
    pub token: Option<String>,
    #[serde(default)]
    pub styles: Vec<LayerTextStyle>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InspectableLayer {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub layer_type: String,
    pub depth: usize,
    pub order: usize,
    pub frame: Option<LayerFrame>,
    #[serde(default)]
    pub frame_is_visual: bool,
    pub opacity: f64,
    pub rotation: f64,
    pub visible: bool,
    pub radius: LayerRadius,
    pub fills: Vec<LayerPaint>,
    pub borders: Vec<LayerBorder>,
    pub shadows: Vec<LayerShadow>,
    pub blurs: Vec<LayerBlur>,
    pub text: Option<LayerText>,
    pub is_asset: bool,
    pub has_slice: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub capture_id: String,
    #[serde(default)]
    pub data_version: u32,
    pub captured_at: u64,
    pub source_url: String,
    pub resolved_url: String,
    pub team_id: String,
    pub project_id: String,
    pub project_name: String,
    pub output_dir: String,
    pub downloaded_count: usize,
    pub failed_count: usize,
    pub designs: Vec<CapturedDesign>,
    #[serde(default)]
    pub slices: Vec<CapturedSlice>,
    #[serde(default)]
    pub slice_downloaded_count: usize,
    #[serde(default)]
    pub slice_failed_count: usize,
    #[serde(default)]
    pub slice_total_count: usize,
    #[serde(default = "default_true")]
    pub slices_complete: bool,
    #[serde(default)]
    pub layers: Vec<InspectableLayer>,
}

fn default_true() -> bool {
    true
}
