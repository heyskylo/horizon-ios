//! Checked compute dispatch directives and device-independent work group sizing.
use crate::{Macros, Preprocessed, eval_condition};

/// The shader's documented work group count directive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorkGroups {
    Fixed([u32; 3]),
    /// Multipliers of render width and height, with one group in Z.
    Render([f32; 2]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputeDispatch {
    pub local_size: [u32; 3],
    pub work_groups: WorkGroups,
}

impl ComputeDispatch {
    /// Reads live, macro-expanded GLSL. Missing local axes default to one; missing work group
    /// directives cover the framebuffer. Invalid directives fail instead of changing the dispatch.
    pub fn parse(source: &Preprocessed) -> Result<Self, String> {
        let tokens: Vec<_> = crate::macros::lex(&source.code)
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect();
        let mut local_size = [1; 3];
        let mut found_local = false;
        for (axis, key) in ["local_size_x", "local_size_y", "local_size_z"]
            .iter()
            .enumerate()
        {
            let positions: Vec<_> = tokens
                .iter()
                .enumerate()
                .filter(|(_, t)| *t == key)
                .map(|(i, _)| i)
                .collect();
            if positions.len() > 1 {
                return Err(format!("duplicate {key}"));
            }
            if let Some(&i) = positions.first() {
                found_local = true;
                if tokens.get(i + 1) != Some(&"=") {
                    return Err(format!("invalid {key}"));
                }
                let end = (i + 2..tokens.len())
                    .find(|&j| matches!(tokens[j], "," | ")"))
                    .ok_or_else(|| format!("invalid {key}"))?;
                local_size[axis] = integer(&tokens[i + 2..end], false)?;
            }
        }
        if !found_local {
            return Err("missing compute local size declaration".into());
        }
        let fixed = directive(&tokens, "ivec3", "workGroups")?;
        let render = directive(&tokens, "vec2", "workGroupsRender")?;
        if fixed.is_some() && render.is_some() {
            return Err("both workGroups and workGroupsRender are declared".into());
        }
        let work_groups = if let Some(args) = fixed {
            let args = components(args, 3)?;
            WorkGroups::Fixed([
                integer(args[0], true)?,
                integer(args[1], true)?,
                integer(args[2], true)?,
            ])
        } else if let Some(args) = render {
            let args = components(args, 2)?;
            let number = |tokens: &[&str]| {
                let text = tokens.join("");
                text.trim_end_matches(['f', 'F'])
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .ok_or_else(|| format!("invalid workGroupsRender component: {text}"))
            };
            WorkGroups::Render([number(args[0])?, number(args[1])?])
        } else {
            WorkGroups::Render([1.0; 2])
        };
        Ok(Self {
            local_size,
            work_groups,
        })
    }

    /// Number of work groups, checked for representable counts. Device-specific dispatch and
    /// local-size limits are checked by the renderer before allocating or submitting a pass.
    pub fn groups(self, render_size: [u32; 2]) -> Option<[u32; 3]> {
        match self.work_groups {
            WorkGroups::Fixed(groups) => Some(groups),
            WorkGroups::Render(scale) => {
                let mut groups = [1; 3];
                for axis in 0..2 {
                    // Multiply at the declared GLSL float precision before rounding.
                    // Promoting 0.1f to f64 first would turn 10 * 0.1f into >1 and
                    // incorrectly dispatch an extra group on exact pixel boundaries.
                    let pixels = f64::from(render_size[axis] as f32 * scale[axis]).ceil();
                    let count = (pixels / f64::from(self.local_size[axis])).ceil();
                    if !count.is_finite() || count < 0.0 || count > f64::from(u32::MAX) {
                        return None;
                    }
                    groups[axis] = count as u32;
                }
                Some(groups)
            }
        }
    }
}

fn integer(tokens: &[&str], zero: bool) -> Result<u32, String> {
    let text = tokens.join("");
    let value = eval_condition(&text, &Macros::default())
        .ok()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| zero || *v != 0);
    value.ok_or_else(|| format!("invalid compute integer: {text}"))
}

fn directive<'a>(
    tokens: &'a [&'a str],
    ty: &str,
    name: &str,
) -> Result<Option<&'a [&'a str]>, String> {
    let positions: Vec<_> = tokens
        .windows(3)
        .enumerate()
        .filter(|(_, t)| t[0] == "const" && t[1] == ty && t[2] == name)
        .map(|(i, _)| i)
        .collect();
    if positions.len() > 1 {
        return Err(format!("duplicate {name}"));
    }
    let Some(&i) = positions.first() else {
        return Ok(None);
    };
    if tokens.get(i + 3) != Some(&"=")
        || tokens.get(i + 4) != Some(&ty)
        || tokens.get(i + 5) != Some(&"(")
    {
        return Err(format!("invalid {name} constructor"));
    }
    let mut depth = 1;
    for j in i + 6..tokens.len() {
        match tokens[j] {
            "(" => depth += 1,
            ")" => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            if tokens.get(j + 1) != Some(&";") {
                return Err(format!("invalid {name} declaration"));
            }
            return Ok(Some(&tokens[i + 6..j]));
        }
    }
    Err(format!("unterminated {name} constructor"))
}

fn components<'a>(tokens: &'a [&'a str], size: usize) -> Result<Vec<&'a [&'a str]>, String> {
    let mut args = Vec::new();
    let (mut start, mut depth) = (0, 0);
    for (i, token) in tokens.iter().enumerate() {
        match *token {
            "(" => depth += 1,
            ")" => depth -= 1,
            "," if depth == 0 => {
                args.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(&tokens[start..]);
    if args.len() == 1 {
        args.resize(size, args[0]);
    }
    if args.len() != size || args.iter().any(|a| a.is_empty()) {
        return Err("invalid compute vector component count".into());
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(code: &str) -> Result<ComputeDispatch, String> {
        ComputeDispatch::parse(&Preprocessed {
            code: code.into(),
            ..Default::default()
        })
    }
    #[test]
    fn dispatch_covers_non_multiple_render_dimensions() {
        let d = parse("layout(local_size_x=16,local_size_y=8) in;").unwrap();
        assert_eq!(d.local_size, [16, 8, 1]);
        assert_eq!(d.groups([1921, 1081]), Some([121, 136, 1]));
        let d = parse("layout(local_size_x=19,local_size_z=5) in; const vec2 workGroupsRender=vec2(1.0f, 0.732f);").unwrap();
        assert_eq!(d.groups([1920, 1080]), Some([102, 791, 1]));
        let d = parse("layout(local_size_x=1) in; const vec2 workGroupsRender=vec2(0.1);").unwrap();
        assert_eq!(d.groups([10, 10]), Some([1, 1, 1]));
    }
    #[test]
    fn solas_voxel_dispatch_is_three_dimensional() {
        let d = parse("layout(local_size_x=8,local_size_y=8,local_size_z=8) in; const ivec3 workGroups=ivec3(16,8,16);").unwrap();
        assert_eq!(d.groups([1920, 1080]), Some([16, 8, 16]));
        assert_eq!(
            parse("layout(local_size_x=4) in; const ivec3 workGroups=ivec3(128/8);")
                .unwrap()
                .groups([1, 1]),
            Some([16; 3])
        );
    }
    #[test]
    fn invalid_dispatch_never_falls_back_to_screen_size() {
        for source in [
            "void main(){}",
            "layout(local_size_x=0) in;",
            "layout(local_size_x=8) in;const ivec3 workGroups=ivec3(-1,2,3);",
            "layout(local_size_x=8) in;const vec2 workGroupsRender=vec2(NaN);",
            "layout(local_size_x=8) in;const ivec3 workGroups=ivec3(1);const vec2 workGroupsRender=vec2(1.0);",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
        let d =
            parse("layout(local_size_x=1) in; const vec2 workGroupsRender=vec2(3e38);").unwrap();
        assert!(d.groups([100, 100]).is_none());
    }
}
