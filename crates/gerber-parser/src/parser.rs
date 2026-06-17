//! Gerber Parser
//! Decodes the most common ones into [`Node`]s. Anything it doesn't
//! recognize becomes [`Node::Unimplemented`] so a single odd command never
//! aborts the whole parse.

use crate::tree::*;

/// Parse Gerber text into a syntax tree.
pub fn parse(source: &str) -> GerberTree {
    let mut children = Vec::new();
    // Coordinate format, needed to decode X/Y string into numbers.
    let mut format: Option<Format> = None;

    for block in split_blocks(source) {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        if let Some(extended) = block.strip_prefix('%').and_then(|b| b.strip_suffix('%')) {
            parse_extended(extended.trim_end_matches('*'), &mut children, &mut format);
        } else {
            parse_word_block(block, &mut children, format);
        }
    }

    GerberTree {
        filetype: Filetype::Gerber,
        children,
    }
}

/// Split source into command blocks. Each Gerber command ends with `*`.
/// Keep surrounding `%` so the caller can tell extended commands apart.
fn split_blocks(source: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut in_extended = false;

    for ch in source.chars() {
        match ch {
            '%' => {
                in_extended = !in_extended;
                current.push(ch);
                if !in_extended {
                    blocks.push(std::mem::take(&mut current));
                }
            }

            '*' if !in_extended => {
                current.push(ch);
                blocks.push(std::mem::take(&mut current));
            }
            '\n' | '\r' => {} // newlines are not significant between blocks
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Parse an extended command (the text between `%` and `%`).
fn parse_extended(body: &str, out: &mut Vec<Node>, format: &mut Option<Format>) {
    if let Some(rest) = body.strip_prefix("MO") {
        // Mode of units: MOMM or MOIN.
        let units = if rest.starts_with("MM") { Units::Millimeters } else { Units::Inches };
        out.push(Node::Units(units));
    } else if let Some(rest) = body.strip_prefix("FS") {
        // Format spec, e.g. FSLAX34Y34. L=leading suppression, A=absolute.
        let zero = if rest.starts_with('L') {
            Some(ZeroSuppression::Leading)
        } else if rest.starts_with('T') {
            Some(ZeroSuppression::Trailing)
        } else {
            None
        };
        let mode = if rest.contains('A') {
            Some(CoordinateMode::Absolute)
        } else if rest.contains('I') {
            Some(CoordinateMode::Incremental)
        } else {
            None
        };
        let parsed = parse_format(rest);
        *format = parsed;
        out.push(Node::CoordinateFormat { format: parsed, zero_suppression: zero, mode });
    } else if let Some(rest) = body.strip_prefix("ADD") {
        // Aperture definition, e.g. ADD10C,0.006 or ADD13R,0.06X0.06.
        if let Some(node) = parse_aperture(rest) {
            out.push(node);
        } else {
            out.push(Node::Unimplemented(format!("%{body}%")));
        }
    } else if let Some(rest) = body.strip_prefix("LP") {
        let polarity = if rest.starts_with('C') { Polarity::Clear } else { Polarity::Dark };
        out.push(Node::LoadPolarity(polarity));
    } else if let Some(node) = parse_attribute(body) {
        out.push(node);
    } else {
        out.push(Node::Unimplemented(format!("%{body}%")));
    }
}

/// Parse a Gerber X2/X3 attribute command body (the text between the `%` signs,
/// trailing `*` already stripped), e.g. `TF.FileFunction,Copper,L1,Top`.
///
/// Returns `None` if `body` isn't a `TF`/`TA`/`TO`/`TD` attribute, so the caller
/// can fall through to other command handling.
fn parse_attribute(body: &str) -> Option<Node> {
    let (kind, rest) = if let Some(r) = body.strip_prefix("TF") {
        (AttributeKind::File, r)
    } else if let Some(r) = body.strip_prefix("TA") {
        (AttributeKind::Aperture, r)
    } else if let Some(r) = body.strip_prefix("TO") {
        (AttributeKind::Object, r)
    } else if let Some(r) = body.strip_prefix("TD") {
        (AttributeKind::Delete, r)
    } else {
        return None;
    };

    // `rest` is `.Name,val1,val2,...`. The name runs up to the first comma.
    let mut parts = rest.splitn(2, ',');
    let name = parts.next().unwrap_or("").trim().to_string();
    let values = parts
        .next()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    Some(Node::Attribute(Attribute { kind, name, values }))
}

/// Extract integer/decimal places from a format spec body like "LAX34Y34".
fn parse_format(rest: &str) -> Option<Format> {
    let x_index = rest.find('X')?;
    let digits: Vec<char> = rest[x_index + 1..].chars().take(2).collect();
    if digits.len() == 2 {
        let integer = digits[0].to_digit(10)? as u8;
        let decimal = digits[1].to_digit(10)? as u8;
        Some(Format { integer, decimal })
    } else {
        None
    }
}

/// Parse the body of an `ADD` command after the "ADD" prefix.
/// e.g. "10C,0.006" or "13R,0.0669X0.0669".
fn parse_aperture(rest: &str) -> Option<Node> {
    // Code is the leading digits.
    let code_len = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if code_len == 0 {
        return None;
    }
    let (code, remainder) = rest.split_at(code_len);
    let shape_char = remainder.chars().next()?;
    // Parameters follow a comma, separated by 'X'.
    let params: Vec<f64> = remainder
        .split_once(',')
        .map(|(_, p)| p.split('X').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default();

    let shape = match shape_char {
        'C' => ToolShape::Circle { diameter: *params.first()? },
        'R' => ToolShape::Rectangle { x_size: *params.first()?, y_size: *params.get(1)? },
        'O' => ToolShape::Obround { x_size: *params.first()?, y_size: *params.get(1)? },
        'P' => ToolShape::Polygon {
            diameter: *params.first()?,
            vertices: *params.get(1)? as u32,
            rotation: params.get(2).copied(),
        },
        _ => return None, // macro-named apertures: TODO
    };
    Some(Node::ToolDefinition { code: code.to_string(), shape, hole: None })
}

/// Parse a non-extended "word" block: a run of letter+number words like
/// `G54D10` or `X40250Y6750D03` or `G01`.
fn parse_word_block(block: &str, out: &mut Vec<Node>, format: Option<Format>) {
    let body = block.trim_end_matches('*');
    let words = split_words(body);

    let mut coords = Coordinates::default();
    let mut graphic: Option<GraphicType> = None;
    let mut has_coords = false;

    for (letter, value) in &words {
        match letter {
            'G' => match value.as_str() {
                "01" | "1" => out.push(Node::InterpolateMode(InterpolateMode::Line)),
                "02" | "2" => out.push(Node::InterpolateMode(InterpolateMode::CwArc)),
                "03" | "3" => out.push(Node::InterpolateMode(InterpolateMode::CcwArc)),
                "36" => out.push(Node::RegionMode(true)),
                "37" => out.push(Node::RegionMode(false)),
                "70" => out.push(Node::Units(Units::Inches)),
                "71" => out.push(Node::Units(Units::Millimeters)),
                "04" => out.push(Node::Comment(String::new())),
                "54" | "55" | "90" => {} // tool-prep / absolute mode: no-op here
                other => out.push(Node::Unimplemented(format!("G{other}"))),
            },
            'D' => match value.as_str() {
                "01" | "1" => graphic = Some(GraphicType::Segment),
                "02" | "2" => graphic = Some(GraphicType::Move),
                "03" | "3" => graphic = Some(GraphicType::Shape),
                code => out.push(Node::ToolChange { code: code.to_string() }),
            },
            'M' => out.push(Node::Done),
            'X' => { coords.x = decode_coord(value, format); has_coords = true; }
            'Y' => { coords.y = decode_coord(value, format); has_coords = true; }
            'I' => { coords.i = decode_coord(value, format); has_coords = true; }
            'J' => { coords.j = decode_coord(value, format); has_coords = true; }
            _ => out.push(Node::Unimplemented(format!("{letter}{value}"))),
        }
    }

    if has_coords || graphic.is_some() {
        out.push(Node::Graphic { graphic, coordinates: coords });
    }
}

/// Split a word block into (letter, value) pairs. A value is the run of digits
/// and signs following a command letter.
fn split_words(body: &str) -> Vec<(char, String)> {
    let mut words = Vec::new();
    let mut chars = body.chars().peekable();
    while let Some(letter) = chars.next() {
        if !letter.is_ascii_alphabetic() {
            continue;
        }
        let mut value = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
                value.push(c);
                chars.next();
            } else {
                break;
            }
        }
        words.push((letter, value));
    }
    words
}

/// Decode a raw coordinate string into a number using the file's format spec.
/// e.g. with format 3.4 and leading-zero suppression, "40250" -> 4.025.
fn decode_coord(raw: &str, format: Option<Format>) -> Option<f64> {
    if raw.is_empty() {
        return None;
    }
    // If the value already contains a decimal point, trust it directly.
    if raw.contains('.') {
        return raw.parse().ok();
    }
    let format = format?;
    let negative = raw.starts_with('-');
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let total = (format.integer + format.decimal) as usize;
    // Pad leading zeros back (assumes leading-zero suppression, the common case).
    let padded = format!("{digits:0>total$}");
    let (int_part, dec_part) = padded.split_at(format.integer as usize);
    let value: f64 = format!("{int_part}.{dec_part}").parse().ok()?;
    Some(if negative { -value } else { value })
}