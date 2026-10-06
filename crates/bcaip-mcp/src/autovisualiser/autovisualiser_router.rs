use etcetera::{AppStrategy, choose_app_strategy};
use indoc::formatdoc;
use rmcp::{
    RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ContentBlock, ErrorCode, ErrorData, Implementation, InitializeResult,
        ListResourcesResult, MetaObject, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ServerCapabilities,
        ServerConfig,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
/// MIME type for MCP Apps (SEP-1865)
const MCP_APPS_MIME_TYPE: &str = "text/html;profile=mcp-app";

/// Build a Meta object with `_meta.ui.resourceUri` for linking a tool to a UI resource.
fn ui_resource_meta(uri: &str) -> MetaObject {
    let mut meta = MetaObject::new();
    meta.0
        .insert("ui".to_string(), json!({ "resourceUri": uri }));
    meta
}

/// Struct representing the UI resource definitions for autovisualiser chart types.
struct UIResourceDef {
    uri: &'static str,
    name: &'static str,
    description: &'static str,
}

const UI_RESOURCES: &[UIResourceDef] = &[
    UIResourceDef {
        uri: "ui://autovisualiser/chart",
        name: "Chart",
        description: "Interactive line, bar, and scatter chart visualization",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/sankey",
        name: "Sankey Diagram",
        description: "Flow diagram showing relationships between nodes",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/radar",
        name: "Radar Chart",
        description: "Multi-dimensional data comparison spider chart",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/donut",
        name: "Donut/Pie Chart",
        description: "Categorical data visualization as donut or pie chart",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/treemap",
        name: "Treemap",
        description: "Hierarchical data visualization with proportional areas",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/chord",
        name: "Chord Diagram",
        description: "Relationship and flow visualization between entities",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/map",
        name: "Interactive Map",
        description: "Geographic data visualization with location markers",
    },
    UIResourceDef {
        uri: "ui://autovisualiser/mermaid",
        name: "Mermaid Diagram",
        description: "Diagram visualization from Mermaid syntax",
    },
];

fn validation_err(msg: impl Into<String>) -> ErrorData {
    ErrorData::new(ErrorCode::INVALID_PARAMS, msg.into(), None)
}

/// Accepts `data` either as a JSON value or as a JSON-encoded string,
/// since models sometimes emit complex tool parameters double-encoded.
fn lenient_data<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    let value = match value {
        Value::String(s) => serde_json::from_str(&s).map_err(|e| {
            serde::de::Error::custom(format!(
                "the 'data' parameter was a JSON-encoded string that could not be parsed as JSON ({e}); provide 'data' as a JSON object, not a string"
            ))
        })?,
        other => other,
    };
    T::deserialize(value).map_err(serde::de::Error::custom)
}

/// Sankey node structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SankeyNode {
    /// The name of the node
    pub name: String,
    /// Optional category for the node
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Sankey link structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SankeyLink {
    /// Source node name
    pub source: String,
    /// Target node name
    pub target: String,
    /// Flow value
    pub value: f64,
}

/// Sankey data structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SankeyData {
    /// Array of nodes
    pub nodes: Vec<SankeyNode>,
    /// Array of links between nodes
    pub links: Vec<SankeyLink>,
}

impl SankeyData {
    fn validate(&self) -> Result<(), ErrorData> {
        if self.nodes.is_empty() {
            return Err(validation_err("nodes array must not be empty"));
        }
        if self.links.is_empty() {
            return Err(validation_err("links array must not be empty"));
        }
        let names: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.name.as_str()).collect();
        for link in &self.links {
            if !names.contains(link.source.as_str()) {
                return Err(validation_err(format!(
                    "link source '{}' not found in nodes",
                    link.source
                )));
            }
            if !names.contains(link.target.as_str()) {
                return Err(validation_err(format!(
                    "link target '{}' not found in nodes",
                    link.target
                )));
            }
            if link.value <= 0.0 {
                return Err(validation_err(format!(
                    "link value must be positive, got {} for '{}' → '{}'",
                    link.value, link.source, link.target
                )));
            }
        }
        Ok(())
    }
}

/// Parameters for render_sankey tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderSankeyParams {
    /// The data for the Sankey diagram
    #[serde(deserialize_with = "lenient_data")]
    pub data: SankeyData,
}

/// Radar dataset structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RadarDataset {
    /// Label for this dataset
    pub label: String,
    /// Data values for each category
    pub data: Vec<f64>,
}

/// Radar chart data structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RadarData {
    /// Category labels
    pub labels: Vec<String>,
    /// Datasets to compare
    pub datasets: Vec<RadarDataset>,
}

impl RadarData {
    fn validate(&self) -> Result<(), ErrorData> {
        if self.labels.is_empty() {
            return Err(validation_err("labels array must not be empty"));
        }
        if self.datasets.is_empty() {
            return Err(validation_err("datasets array must not be empty"));
        }
        let expected = self.labels.len();
        for ds in &self.datasets {
            if ds.data.len() != expected {
                return Err(validation_err(format!(
                    "dataset '{}' has {} values but there are {} labels",
                    ds.label,
                    ds.data.len(),
                    expected
                )));
            }
        }
        Ok(())
    }
}

/// Parameters for render_radar tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderRadarParams {
    /// The data for the radar chart
    #[serde(deserialize_with = "lenient_data")]
    pub data: RadarData,
}

/// Data item for donut/pie charts - can be a number or labeled value
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[serde(untagged)]
pub enum DonutDataItem {
    /// Simple numeric value
    Number(f64),
    /// Labeled value with explicit label
    LabeledValue {
        /// Label for this data point
        label: String,
        /// Numeric value
        value: f64,
    },
}

/// Chart type for donut/pie charts
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DonutChartType {
    /// Doughnut chart (with hole in center)
    Doughnut,
    /// Pie chart (no hole)
    Pie,
}

/// Single donut/pie chart data
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct SingleDonutChart {
    /// Array of values — numbers (e.g. [10, 20]) or objects (e.g. [{"label": "A", "value": 10}])
    pub values: Vec<DonutDataItem>,
    /// Optional chart title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional chart type (doughnut or pie)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub chart_type: Option<DonutChartType>,
    /// Optional labels array (used when values are just numbers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
}

impl SingleDonutChart {
    fn validate(&self) -> Result<(), ErrorData> {
        if self.values.is_empty() {
            return Err(validation_err("values array must not be empty"));
        }
        if let Some(labels) = &self.labels
            && labels.len() != self.values.len()
        {
            return Err(validation_err(format!(
                "labels array length ({}) must match values array length ({})",
                labels.len(),
                self.values.len()
            )));
        }
        Ok(())
    }
}

fn validate_donut_charts(charts: &[SingleDonutChart]) -> Result<(), ErrorData> {
    if charts.is_empty() {
        return Err(validation_err("charts array must not be empty"));
    }
    for chart in charts {
        chart.validate()?;
    }
    Ok(())
}

/// Parameters for render_donut tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderDonutParams {
    /// The chart data as an array of chart objects. Use a single-element array for one chart.
    #[serde(deserialize_with = "lenient_data")]
    pub data: Vec<SingleDonutChart>,
}

/// Treemap node structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct TreemapNode {
    /// Name of the node
    pub name: String,
    /// Value for leaf nodes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Category for coloring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Children nodes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreemapNode>>,
}

impl TreemapNode {
    fn validate(&self) -> Result<(), ErrorData> {
        // Must have either a value or children
        if self.value.is_none() && self.children.as_ref().is_none_or(|c| c.is_empty()) {
            return Err(validation_err(format!(
                "node '{}' must have either a value or non-empty children",
                self.name
            )));
        }
        if let Some(children) = &self.children {
            for child in children {
                child.validate()?;
            }
        }
        Ok(())
    }
}

/// Parameters for render_treemap tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderTreemapParams {
    /// The hierarchical data for the treemap
    #[serde(deserialize_with = "lenient_data")]
    pub data: TreemapNode,
}

/// Chord diagram data structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ChordData {
    /// Labels for each entity
    pub labels: Vec<String>,
    /// 2D matrix of flows (matrix[i][j] = flow from i to j)
    pub matrix: Vec<Vec<f64>>,
}

impl ChordData {
    fn validate(&self) -> Result<(), ErrorData> {
        if self.labels.is_empty() {
            return Err(validation_err("labels array must not be empty"));
        }
        let n = self.labels.len();
        if self.matrix.len() != n {
            return Err(validation_err(format!(
                "matrix has {} rows but there are {} labels",
                self.matrix.len(),
                n
            )));
        }
        for (i, row) in self.matrix.iter().enumerate() {
            if row.len() != n {
                return Err(validation_err(format!(
                    "matrix row {} has {} columns but expected {}",
                    i,
                    row.len(),
                    n
                )));
            }
        }
        Ok(())
    }
}

/// Parameters for render_chord tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderChordParams {
    /// The data for the chord diagram
    #[serde(deserialize_with = "lenient_data")]
    pub data: ChordData,
}

/// Map marker structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct MapMarker {
    /// Latitude (required)
    pub lat: f64,
    /// Longitude (required)
    pub lng: f64,
    /// Location name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Numeric value for sizing/coloring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Description text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Custom popup HTML
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popup: Option<String>,
    /// Custom marker color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Custom marker label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Use default Leaflet icon
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "useDefaultIcon")]
    pub use_default_icon: Option<bool>,
}

/// Map center point
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct MapCenter {
    /// Latitude
    pub lat: f64,
    /// Longitude
    pub lng: f64,
}

/// Map data structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct MapData {
    /// Array of markers
    pub markers: Vec<MapMarker>,
    /// Optional title for the map
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional subtitle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Optional center point
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<MapCenter>,
    /// Optional initial zoom level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    /// Optional boolean to enable/disable clustering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clustering: Option<bool>,
    /// Optional cluster radius
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clusterRadius")]
    pub cluster_radius: Option<f64>,
    /// Optional boolean to auto-fit map to markers
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "autoFit")]
    pub auto_fit: Option<bool>,
}

impl MapData {
    fn validate(&self) -> Result<(), ErrorData> {
        if self.markers.is_empty() {
            return Err(validation_err("markers array must not be empty"));
        }
        for (i, m) in self.markers.iter().enumerate() {
            if !(-90.0..=90.0).contains(&m.lat) {
                return Err(validation_err(format!(
                    "marker {} has invalid latitude {} (must be -90 to 90)",
                    i, m.lat
                )));
            }
            if !(-180.0..=180.0).contains(&m.lng) {
                return Err(validation_err(format!(
                    "marker {} has invalid longitude {} (must be -180 to 180)",
                    i, m.lng
                )));
            }
        }
        Ok(())
    }
}

/// Parameters for render_map tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderMapParams {
    /// The data for the map visualization
    #[serde(deserialize_with = "lenient_data")]
    pub data: MapData,
}

/// Chart data point for scatter charts
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ChartPoint {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
}

/// Chart dataset structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ChartDataset {
    /// Label for this dataset
    pub label: String,
    /// Data points - can be numbers or x/y points
    pub data: ChartDataValues,
    /// Optional background color for the dataset
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "backgroundColor")]
    pub background_color: Option<String>,
    /// Optional border color for the dataset
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "borderColor")]
    pub border_color: Option<String>,
    /// Optional border width for the dataset
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "borderWidth")]
    pub border_width: Option<f64>,
    /// Optional tension for line curves (0 = straight lines, higher = more curved)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tension: Option<f64>,
    /// Optional fill setting for area under the line
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<bool>,
}

/// Chart data values - can be simple numbers or x/y points
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[serde(untagged)]
pub enum ChartDataValues {
    /// Simple numeric values (for line/bar charts with labels)
    Numbers(Vec<f64>),
    /// X/Y points (for scatter charts or line charts without labels)
    Points(Vec<ChartPoint>),
}

/// Chart type enumeration
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChartType {
    /// Line chart
    Line,
    /// Scatter chart
    Scatter,
    /// Bar chart
    Bar,
}

/// Chart data structure
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ChartData {
    /// Chart type
    #[serde(rename = "type")]
    pub chart_type: ChartType,
    /// Datasets to display
    pub datasets: Vec<ChartDataset>,
    /// Optional labels for x-axis (for line/bar charts)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// Optional chart title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional subtitle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Optional x-axis label
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "xAxisLabel")]
    pub x_axis_label: Option<String>,
    /// Optional y-axis label
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "yAxisLabel")]
    pub y_axis_label: Option<String>,
}

impl ChartData {
    fn validate(&self) -> Result<(), ErrorData> {
        if self.datasets.is_empty() {
            return Err(validation_err("datasets array must not be empty"));
        }
        if let Some(labels) = &self.labels {
            for ds in &self.datasets {
                if let ChartDataValues::Numbers(nums) = &ds.data
                    && nums.len() != labels.len()
                {
                    return Err(validation_err(format!(
                        "dataset '{}' has {} values but there are {} labels",
                        ds.label,
                        nums.len(),
                        labels.len()
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Parameters for show_chart tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ShowChartParams {
    /// The data for the chart
    #[serde(deserialize_with = "lenient_data")]
    pub data: ChartData,
}

/// Parameters for render_mermaid tool
#[derive(Debug, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RenderMermaidParams {
    /// The Mermaid diagram code to render
    pub mermaid_code: String,
}

/// An extension for automatic data visualization and UI generation
#[derive(Clone)]
pub struct AutoVisualiserRouter {
    tool_router: ToolRouter<Self>,
    #[allow(dead_code)]
    cache_dir: PathBuf,
    instructions: String,
}

impl Default for AutoVisualiserRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AutoVisualiserRouter {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "goose-autovisualiser",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(self.instructions.clone())
    }

    async fn list_resources(
        &self,
        _pagination: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let resources = UI_RESOURCES
            .iter()
            .map(|def| {
                Resource::new(def.uri, def.name)
                    .with_title(def.name)
                    .with_description(def.description)
                    .with_mime_type(MCP_APPS_MIME_TYPE)
            })
            .collect();

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let html = self.get_template_html(&params.uri)?;

        let mut meta = MetaObject::new();
        meta.0
            .insert("ui".to_string(), json!({ "prefersBorder": true }));

        let resource_contents = ResourceContents::TextResourceContents {
            uri: params.uri,
            mime_type: Some(MCP_APPS_MIME_TYPE.to_string()),
            text: html,
            meta: Some(meta),
        };

        Ok(ReadResourceResult::new(vec![resource_contents]).into())
    }
}

#[tool_router(router = tool_router)]
impl AutoVisualiserRouter {
    pub fn new() -> Self {
        // choose_app_strategy().cache_dir()
        // - macOS/Linux: ~/.cache/goose/autovisualiser/
        // - Windows:     ~\AppData\Local\Block\goose\cache\autovisualiser\
        let cache_dir = choose_app_strategy(crate::APP_STRATEGY.clone())
            .unwrap()
            .cache_dir()
            .join("autovisualiser");

        // Create cache directory if it doesn't exist
        let _ = std::fs::create_dir_all(&cache_dir);

        let instructions = formatdoc! {r#"
            This extension provides tools for automatic data visualization
            Use these tools when you are presenting data to the user which could be complemented by a visual expression
            Choose the most appropriate chart type based on the data you have and can provide
            It is important you match the data format as appropriate with the chart type you have chosen
            The user may specify a type of chart or you can pick one of the most appropriate that you can shape the data to

            ## Available Tools:
            - **render_sankey**: Creates interactive Sankey diagrams from flow data
            - **render_radar**: Creates interactive radar charts for multi-dimensional data comparison
            - **render_donut**: Creates interactive donut/pie charts for categorical data (supports multiple charts)
            - **render_treemap**: Creates interactive treemap visualizations for hierarchical data
            - **render_chord**: Creates interactive chord diagrams for relationship/flow visualization
            - **render_map**: Creates interactive map visualizations with location markers
            - **render_mermaid**: Creates interactive Mermaid diagrams from Mermaid syntax
            - **show_chart**: Creates interactive line, scatter, or bar charts for data visualization
        "#};

        Self {
            tool_router: Self::tool_router(),
            cache_dir,
            instructions,
        }
    }

    /// Get the static HTML template for a given `ui://` resource URI.
    /// Templates have JS libs inlined but NO data baked in — data arrives via postMessage.
    fn get_template_html(&self, uri: &str) -> Result<String, ErrorData> {
        match uri {
            "ui://autovisualiser/chart" => {
                const TEMPLATE: &str = include_str!("templates/chart_template.html");
                const CHART_MIN: &str = include_str!("templates/assets/chart.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{CHART_MIN}}", CHART_MIN)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/sankey" => {
                const TEMPLATE: &str = include_str!("templates/sankey_template.html");
                const D3_MIN: &str = include_str!("templates/assets/d3.min.js");
                const D3_SANKEY: &str = include_str!("templates/assets/d3.sankey.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{D3_MIN}}", D3_MIN)
                    .replace("{{D3_SANKY}}", D3_SANKEY)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/radar" => {
                const TEMPLATE: &str = include_str!("templates/radar_template.html");
                const CHART_MIN: &str = include_str!("templates/assets/chart.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{CHART_MIN}}", CHART_MIN)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/donut" => {
                const TEMPLATE: &str = include_str!("templates/donut_template.html");
                const CHART_MIN: &str = include_str!("templates/assets/chart.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{CHART_MIN}}", CHART_MIN)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/treemap" => {
                const TEMPLATE: &str = include_str!("templates/treemap_template.html");
                const D3_MIN: &str = include_str!("templates/assets/d3.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{D3_MIN}}", D3_MIN)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/chord" => {
                const TEMPLATE: &str = include_str!("templates/chord_template.html");
                const D3_MIN: &str = include_str!("templates/assets/d3.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{D3_MIN}}", D3_MIN)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/map" => {
                const TEMPLATE: &str = include_str!("templates/map_template.html");
                const LEAFLET_JS: &str = include_str!("templates/assets/leaflet.min.js");
                const LEAFLET_CSS: &str = include_str!("templates/assets/leaflet.min.css");
                const MARKERCLUSTER_JS: &str =
                    include_str!("templates/assets/leaflet.markercluster.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{LEAFLET_JS}}", LEAFLET_JS)
                    .replace("{{LEAFLET_CSS}}", LEAFLET_CSS)
                    .replace("{{MARKERCLUSTER_JS}}", MARKERCLUSTER_JS)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            "ui://autovisualiser/mermaid" => {
                const TEMPLATE: &str = include_str!("templates/mermaid_template.html");
                const MERMAID_MIN: &str = include_str!("templates/assets/mermaid.min.js");
                const BASE_CSS: &str = include_str!("templates/assets/mcp-app-base.css");
                const BRIDGE_JS: &str = include_str!("templates/assets/mcp-app-bridge.js");
                Ok(TEMPLATE
                    .replace("{{MERMAID_MIN}}", MERMAID_MIN)
                    .replace("{{MCP_APP_BASE_CSS}}", BASE_CSS)
                    .replace("{{MCP_APP_BRIDGE}}", BRIDGE_JS))
            }
            _ => Err(ErrorData::new(
                ErrorCode::INVALID_REQUEST,
                format!("Unknown resource URI: {}", uri),
                None,
            )),
        }
    }

    /// show a Sankey diagram from flow data
    #[tool(
        name = "render_sankey",
        description = r#"show a Sankey diagram from flow data
The data must contain:
- nodes: Array of objects with 'name' and optional 'category' properties
- links: Array of objects with 'source', 'target', and 'value' properties

IMPORTANT: Links must NOT form cycles (e.g. A→B and B→A). Sankey diagrams are
directional acyclic flows. If the data has circular relationships, restructure
them so flow moves in one direction (e.g. add a separate node like "Re-signed"
instead of linking back to an earlier node).

Example:
{
  "nodes": [
    {"name": "Source A", "category": "source"},
    {"name": "Target B", "category": "target"}
  ],
  "links": [
    {"source": "Source A", "target": "Target B", "value": 100}
  ]
}"#,
        meta = ui_resource_meta("ui://autovisualiser/sankey")
    )]
    pub async fn render_sankey(
        &self,
        params: Parameters<RenderSankeyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        inner.data.validate()?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        let node_count = data
            .get("nodes")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let link_count = data
            .get("links")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let text_fallback = format!(
            "sankey diagram: {} node(s), {} link(s)",
            node_count, link_count
        );

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/sankey")));

        Ok(result)
    }

    /// show a radar chart (spider chart) for multi-dimensional data comparison
    #[tool(
        name = "render_radar",
        description = r#"show a radar chart (spider chart) for multi-dimensional data comparison

The data must contain:
- labels: Array of strings representing the dimensions/axes
- datasets: Array of dataset objects with 'label' and 'data' properties

Example:
{
  "labels": ["Speed", "Strength", "Endurance", "Agility", "Intelligence"],
  "datasets": [
    {
      "label": "Player 1",
      "data": [85, 70, 90, 75, 80]
    },
    {
      "label": "Player 2",
      "data": [75, 85, 80, 90, 70]
    }
  ]
}"#,
        meta = ui_resource_meta("ui://autovisualiser/radar")
    )]
    pub async fn render_radar(
        &self,
        params: Parameters<RenderRadarParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        inner.data.validate()?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        let label_count = data
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let dataset_count = data
            .get("datasets")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let text_fallback = format!(
            "radar chart: {} dimension(s), {} dataset(s)",
            label_count, dataset_count
        );

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/radar")));

        Ok(result)
    }

    /// show pie or donut charts for categorical data visualization
    #[tool(
        name = "render_donut",
        description = r#"show pie or donut charts for categorical data visualization.
Supports one or more charts in a grid layout.
The `data` field must always be an array; pass a single-element array for one chart.

Each chart object must contain:
- values: Array of numbers OR objects with 'label' and 'value'
- type: Optional 'doughnut' (default) or 'pie'
- title: Optional chart title
- labels: Optional array of labels (required when values are plain numbers)

Example single chart (labeled values):
[
  {
    "values": [
      {"label": "Marketing", "value": 25000},
      {"label": "Development", "value": 35000}
    ],
    "title": "Budget"
  }
]

Example single chart (parallel arrays):
[
  {
    "values": [45000, 38000],
    "labels": ["Product A", "Product B"],
    "type": "pie"
  }
]

Example multiple charts (array of chart objects):
[
  {"values": [60, 40], "labels": ["Yes", "No"], "title": "Q1"},
  {"values": [75, 25], "labels": ["Yes", "No"], "title": "Q2"}
]"#,
        meta = ui_resource_meta("ui://autovisualiser/donut")
    )]
    pub async fn render_donut(
        &self,
        params: Parameters<RenderDonutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        validate_donut_charts(&inner.data)?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        let charts = data.as_array().ok_or_else(|| {
            ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "The 'data' parameter must be an array.".to_string(),
                None,
            )
        })?;
        let text_fallback = if charts.len() == 1 {
            let title = charts[0]
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            format!("donut/pie chart: \"{}\"", title)
        } else {
            format!("donut/pie chart: {} chart(s)", charts.len())
        };

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/donut")));

        Ok(result)
    }

    /// show a treemap visualization for hierarchical data
    #[tool(
        name = "render_treemap",
        description = r#"show a treemap visualization for hierarchical data with proportional area representation as boxes

The data should be a hierarchical structure with:
- name: Name of the node (required)
- value: Numeric value for leaf nodes (optional for parent nodes)
- children: Array of child nodes (optional)
- category: Category for coloring (optional)

Example:
{
  "name": "Root",
  "children": [
    {
      "name": "Group A",
      "children": [
        {"name": "Item 1", "value": 100, "category": "Type1"},
        {"name": "Item 2", "value": 200, "category": "Type2"}
      ]
    },
    {"name": "Item 3", "value": 150, "category": "Type1"}
  ]
}"#,
        meta = ui_resource_meta("ui://autovisualiser/treemap")
    )]
    pub async fn render_treemap(
        &self,
        params: Parameters<RenderTreemapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        inner.data.validate()?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        let root_name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Root");
        let child_count = data
            .get("children")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let text_fallback = format!(
            "treemap: \"{}\" with {} top-level children",
            root_name, child_count
        );

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/treemap")));

        Ok(result)
    }

    /// Show a chord diagram visualization for relationships and flows
    #[tool(
        name = "render_chord",
        description = r#"Show a chord diagram visualization for showing relationships and flows between entities.

The data must contain:
- labels: Array of strings representing the entities
- matrix: 2D array of numbers representing flows (matrix[i][j] = flow from i to j)

Example:
{
  "labels": ["North America", "Europe", "Asia", "Africa"],
  "matrix": [
    [0, 15, 25, 8],
    [18, 0, 20, 12],
    [22, 18, 0, 15],
    [5, 10, 18, 0]
  ]
}"#,
        meta = ui_resource_meta("ui://autovisualiser/chord")
    )]
    pub async fn render_chord(
        &self,
        params: Parameters<RenderChordParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        inner.data.validate()?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        let entity_count = data
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let text_fallback = format!("chord diagram: {} entities", entity_count);

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/chord")));

        Ok(result)
    }

    /// show an interactive map visualization with location markers
    #[tool(
        name = "render_map",
        description = r#"show an interactive map visualization with location markers using Leaflet.

The data must contain:
- markers: Array of objects with 'lat', 'lng', and optional properties
- title: Optional title for the map (default: "Interactive Map")
- subtitle: Optional subtitle (default: "Geographic data visualization")
- center: Optional center point {lat, lng} (default: USA center)
- zoom: Optional initial zoom level (default: 4)
- clustering: Optional boolean to enable/disable clustering (default: true)
- autoFit: Optional boolean to auto-fit map to markers (default: true)

Marker properties:
- lat: Latitude (required)
- lng: Longitude (required)
- name: Location name
- value: Numeric value for sizing/coloring
- description: Description text
- popup: Custom popup HTML
- color: Custom marker color
- label: Custom marker label
- useDefaultIcon: Use default Leaflet icon

Example:
{
  "title": "Store Locations",
  "markers": [
    {"lat": 37.7749, "lng": -122.4194, "name": "SF Store", "value": 150000},
    {"lat": 40.7128, "lng": -74.0060, "name": "NYC Store", "value": 200000}
  ]
}"#,
        meta = ui_resource_meta("ui://autovisualiser/map")
    )]
    pub async fn render_map(
        &self,
        params: Parameters<RenderMapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        inner.data.validate()?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        let title = data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Interactive Map");
        let marker_count = data
            .get("markers")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let text_fallback = format!("map: \"{}\" with {} marker(s)", title, marker_count);

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/map")));

        Ok(result)
    }

    /// show a Mermaid diagram from Mermaid syntax
    #[tool(
        name = "render_mermaid",
        description = r#"show a Mermaid diagram from Mermaid syntax

Provide the Mermaid code as a string. Supports flowcharts, sequence diagrams, Gantt charts, etc.

Example:
graph TD;
    A-->B;
    A-->C;
    B-->D;
    C-->D;
"#,
        meta = ui_resource_meta("ui://autovisualiser/mermaid")
    )]
    pub async fn render_mermaid(
        &self,
        params: Parameters<RenderMermaidParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mermaid_code = &params.0.mermaid_code;

        let first_line = mermaid_code.lines().next().unwrap_or("diagram").trim();
        let text_fallback = format!("mermaid diagram: {}", first_line);

        let data = serde_json::to_value(&params.0).map_err(|e| {
            ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("Invalid parameters: {}", e),
                None,
            )
        })?;

        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/mermaid")));

        Ok(result)
    }

    /// show interactive line, scatter, or bar charts
    #[tool(
        name = "show_chart",
        description = r#"show interactive line, scatter, or bar charts

Required: type ('line', 'scatter', or 'bar'), datasets array
Optional: labels, title, subtitle, xAxisLabel, yAxisLabel, options

Example:
{
  "type": "line",
  "title": "Monthly Sales",
  "labels": ["Jan", "Feb", "Mar"],
  "datasets": [
    {"label": "Product A", "data": [65, 59, 80]}
  ]
}"#,
        meta = ui_resource_meta("ui://autovisualiser/chart")
    )]
    pub async fn show_chart(
        &self,
        params: Parameters<ShowChartParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let inner = params.0;
        inner.data.validate()?;
        let data = serde_json::to_value(inner.data)
            .map_err(|e| validation_err(format!("Invalid parameters: {e}")))?;

        // Build a text fallback describing the chart for non-UI hosts
        let chart_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("chart");
        let title = data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled");
        let dataset_count = data
            .get("datasets")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let text_fallback = format!(
            "{} chart: \"{}\" with {} dataset(s)",
            chart_type, title, dataset_count
        );

        // Return structuredContent (the raw data) + text fallback.
        // The host fetches the template via read_resource and sends this data
        // to the template via the MCP Apps postMessage lifecycle.
        let mut result = CallToolResult::structured(data);
        result.content = vec![ContentBlock::text(text_fallback)];
        result = result.with_meta(Some(ui_resource_meta("ui://autovisualiser/chart")));

        Ok(result)
    }
}
