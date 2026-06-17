//! Gerber syntax tree

/// Parsed file is a Gerber or a NC Drill
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType{
    Gerber,
    Drill
}

/// Unit Of Measure
#[debug(Debug, Clone, Copy, PartialEq, Eq)]
pub enum units {
    Millimeters,
    Inches
}

/// Coordinate string format: (integer places, decimal places)
/// e.g. `FSLAX34Y34` -> `Format { integer: 3, decimal: 4 }`.
#[derive(Debug, Clone, Copy, PartialEq,Eq)]
pub struct Format { 
    pub integer: u8,
    pub decimal: u8,
}

/// Which zeors are omitted from coordiante string
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroSuppression{
    Leading,
    Trailing
}

/// Absolute vs incremental coordinates.
/// This is deprecated and rare but still out there
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateMode{
    Absolute,
    Incremental
}

/// A tool's outer shape
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolShape{
    Circle { diameter: f64 },
    Rectangle { x_size: f64, y_size: f64 },
    Obround { x_size: f64, y_size: f64 },
    Polygon { diameter: f64, vertices: u32, rotation: Option<f64> },
    Macro { name: String, variable_values: Vec<f64> },
}

/// Tool hole in center (circle or rectangle)
#[derive(Debug, Clone, PartailEq)]
pub enum HoleShape {
    Circle {diameter: f64},
    Rectangle {x_size: f64, y_size: f64}
}

/// Graphic Operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicType {
    /// Flash the current tool's shape (a pad or drill hit).
    Shape,
    /// Move the plotter without drawing.
    Move,
    /// Stroke the tool from the current point to the coordinates.
    Segment,
    /// Drill-file slot from (x1,y1) to (x2,y2).
    Slot,
}

/// Segments between points
#[derive(Debug, Clone, Copy, PartailEq, Eq)]
pub enum InterpolateMode {
    Line,
    CwArc,
    CcArc,
    Move,
    Drill,
}

/// Image Polarity
#[derive(Debug, Clone, Copy, PartailEq, Eq)]
pub enum Polarity {
    Dark,
    Clear,
}

/// Coordinates for a graphic operation.
#[derive(Debug, Clone, Copy, Default, PartailEq)]
pub struct Coordinates {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub i: Option<f64>,
    pub j: Option<f64>,
}

/// Node in the syntax tree
#[derive(Debug, Clone, PartailEq)]
pub enum Node {
    /// `G04` / drill comment. Usually ignorable.
    Comment(String),
    /// `M00`/`M02`/`M30` — file is complete.
    Done,
    /// `%MOMM*%` / `%MOIN*%` or `G70`/`G71`.
    Units(Units),
    /// `%FSLAX34Y34*%`.
    CoordinateFormat {
        format: Option<Format>,
        zero_suppression: Option<ZeroSuppression>,
        mode: Option<CoordinateMode>,
    },
    /// `%ADD10C,0.006*%` — define tool `code` with a shape and optional hole.
    ToolDefinition {
        code: String,
        shape: ToolShape,
        hole: Option<HoleShape>,
    },
    /// `D10` / `G54D10` — make tool `code` the active tool.
    ToolChange { code: String },
    /// `%LPD*%` / `%LPC*%`.
    LoadPolarity(Polarity),
    /// `G01`/`G02`/`G03` (and drill move/drill modes).
    InterpolateMode(InterpolateMode),
    /// `G36`/`G37` — start/stop a region fill.
    RegionMode(bool),
    /// A draw/move/flash operation, e.g. `X40250Y6750D03`.
    Graphic {
        graphic: Option<GraphicType>,
        coordinates: Coordinates,
    },
    /// A Gerber X2/X3 attribute command: `%TF.*%`, `%TA.*%`, `%TO.*%`, `%TD.*%`.
    /// These carry metadata (layer function, net names, component data) rather
    /// than geometry. See [`Attribute`].
    Attribute(Attribute),
    /// A recognized chunk
    Unimplemented(String),
}

/// Which attribute namespace a Gerber X2/X3 attribute belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeKind {
    /// `%TF` — File attribute (applies to the whole file, e.g. `.FileFunction`).
    File,
    /// `%TA` — Aperture attribute (applies to the current aperture).
    Aperture,
    /// `%TO` — Object attribute (applies to following objects, e.g. `.N` net,
    /// `.C` component reference designator).
    Object,
    /// `%TD` — Delete attribute (clears a previously set attribute).
    Delete,
}

/// A parsed Gerber X2/X3 attribute, e.g. `%TF.FileFunction,Copper,L1,Top*%`
/// becomes `{ kind: File, name: ".FileFunction", values: ["Copper","L1","Top"] }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub kind: AttributeKind,
    /// The attribute name including its leading dot, e.g. `.FileFunction`, `.N`.
    pub name: String,
    /// Comma-separated values after the name.
    pub values: Vec<String>,
}

/// The detected Gerber dialect, by the metadata a file carries. Ordered so
/// `<` comparisons mean "older / less metadata".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GerberVersion {
    /// Plain RS-274-X: no `%TF/%TA/%TO` attributes.
    X1Legacy,
    /// X2: has fabrication attributes (e.g. `.FileFunction`), but no assembly
    /// (component) data.
    X2Fab,
    /// X3: also carries component/assembly attributes (`.C`, assembly layers).
    X3Assembly,
}

/// Root of the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct GerberTree {
    pub filetype: Filetype,
    pub children: Vec<Node>,
}

impl GerberTree {
    /// Detect which Gerber dialect this file uses, by scanning its attributes.
    ///
    /// Upgrades from [`GerberVersion::X1Legacy`] as evidence appears: any
    /// fabrication attribute makes it X2; component/assembly evidence makes it
    /// X3 (an `.FileFunction` mentioning "Assembly", or a `%TO.C` component tag).
    pub fn version(&self) -> GerberVersion {
        let mut version = GerberVersion::X1Legacy;
        for node in &self.children {
            if let Node::Attribute(attr) = node {
                match attr.kind {
                    AttributeKind::File if attr.name == ".FileFunction" => {
                        // Assembly-related file functions (AssemblyDrawing,
                        // Component, …) mark a component/assembly file -> X3.
                        let assembly = attr.values.iter().any(|v| {
                            let v = v.to_ascii_lowercase();
                            v.starts_with("assembly") || v == "component"
                        });
                        if assembly {
                            return GerberVersion::X3Assembly;
                        }
                        version = version.max(GerberVersion::X2Fab);
                    }
                    // A component object tag (`%TO.C,<refdes>`) is assembly data.
                    AttributeKind::Object if attr.name == ".C" => {
                        return GerberVersion::X3Assembly;
                    }
                    // Any other attribute is still X2-level metadata.
                    AttributeKind::File | AttributeKind::Aperture | AttributeKind::Object => {
                        version = version.max(GerberVersion::X2Fab);
                    }
                    AttributeKind::Delete => {}
                }
            }
        }
        version
    }

    /// The value of the file's `%TF.FileFunction` attribute, if present — the
    /// X2 way to know what layer a file represents (e.g. `["Copper","L1","Top"]`).
    pub fn file_function(&self) -> Option<&[String]> {
        self.children.iter().find_map(|n| match n {
            Node::Attribute(a)
                if a.kind == AttributeKind::File && a.name == ".FileFunction" =>
            {
                Some(a.values.as_slice())
            }
            _ => None,
        })
    }
}

