//! Gerber parser — turns Gerber file text into a syntax tree.

pub mod parser;
pub mod tree;

pub use parser::parse;
pub use tree::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units_and_format() {
        let tree = parse("%MOIN*%\n%FSLAX34Y34*%");
        assert_eq!(tree.children[0], Node::Units(Units::Inches));
        match &tree.children[1] {
            Node::CoordinateFormat { format, .. } => {
                assert_eq!(*format, Some(Format { integer: 3, decimal: 4 }));
            }
            other => panic!("expected coordinate format, got {other:?}"),
        }
    }

    #[test]
    fn parses_aperture_definition() {
        let tree = parse("%ADD10C,0.006*%");
        assert_eq!(
            tree.children[0],
            Node::ToolDefinition {
                code: "10".into(),
                shape: ToolShape::Circle { diameter: 0.006},
                hole: None,
            }
        );
    }
    #[test]
    fn decodes_coordinates_with_format() {
        // format 3.4, "X40250Y6750D03" -> flash at (4.025, 0.675)
        let tree = parse("%FSLAX34Y34*%\nX40250Y6750D03*");
        let graphic = tree
            .children
            .iter()
            .find_map(|n| match n {
                Node::Graphic { graphic, coordinates } => Some((*graphic, *coordinates)),
                _ => None,
            })
            .expect("a graphic node");
        assert_eq!(graphic.0, Some(GraphicType::Shape));
        assert_eq!(graphic.1.x, Some(4.025));
        assert_eq!(graphic.1.y, Some(0.675));
    }

    #[test]
    fn parses_file_function_attribute() {
        let tree = parse("%TF.FileFunction,Copper,L1,Top*%");
        assert_eq!(
            tree.children[0],
            Node::Attribute(Attribute {
                kind: AttributeKind::File,
                name: ".FileFunction".into(),
                values: vec!["Copper".into(), "L1".into(), "Top".into()],
            })
        );
    }

    #[test]
    fn detects_x1_x2_and_x3() {
        // No attributes -> legacy X1.
        assert_eq!(parse("%FSLAX34Y34*%").version(), GerberVersion::X1Legacy);

        // Fabrication FileFunction -> X2.
        let x2 = parse("%TF.FileFunction,Copper,L1,Top*%\n%FSLAX34Y34*%");
        assert_eq!(x2.version(), GerberVersion::X2Fab);
        assert_eq!(x2.file_function(), Some(&["Copper".into(), "L1".into(), "Top".into()][..]));

        // Component object tag -> X3.
        let x3 = parse("%TF.FileFunction,Copper,L1,Top*%\n%TO.C,R1*%\nD10*");
        assert_eq!(x3.version(), GerberVersion::X3Assembly);

        // An assembly-related FileFunction (AssemblyDrawing, Component, …) -> X3.
        let assy = parse("%TF.FileFunction,AssemblyDrawing,Top*%");
        assert_eq!(assy.version(), GerberVersion::X3Assembly);
    }
}