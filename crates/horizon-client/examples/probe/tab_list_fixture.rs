//! Headless transport for the player list parity gate: builds each case's
//! scoreboard (teams, `list` objective, scores), listed players, header and
//! footer, replays its frames through `horizon_client::tab_list` and writes
//! the draw calls in vanilla's GuiGraphicsExtractor terms (text as styled
//! runs, faces as skin blits), each face's skin chosen by
//! `horizon_client::skins` from the profile's `textures` property.
#![forbid(unsafe_code)]
use horizon_client::{
    component,
    hud::HudAtlas,
    hud_layout::GuiDraw,
    lang,
    scoreboard::{NumberFormat, RenderType, Scoreboard},
    skins,
    tab_list::{self, TabInputs, TabList, TabPlayer},
    text::Text,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufWriter, Write},
    path::PathBuf,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn text(v: &Value) -> Option<Text> {
    (!v.is_null()).then(|| component::to_text(v, &mut |_| Text::empty()))
}
fn format(v: &Value) -> Option<NumberFormat> {
    match v["type"].as_str()? {
        "blank" => Some(NumberFormat::Blank),
        "styled" => Some(NumberFormat::Styled(v["style"].clone())),
        "fixed" => Some(NumberFormat::Fixed(v["value"].clone())),
        _ => None,
    }
}
fn runs(t: &Text) -> Value {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (c, s) in t.chars() {
        let key = json!([
            s.color.map_or(-1, i64::from),
            s.is_bold(),
            s.is_italic(),
            s.underlined == Some(true),
            s.strikethrough == Some(true),
            s.obfuscated == Some(true)
        ]);
        match out.last_mut() {
            Some((run, k)) if *k == key => run.push(c),
            _ => out.push((c.to_string(), key)),
        }
    }
    out.into_iter().map(|(r, k)| json!([r, k])).collect()
}

pub fn main() -> Result<()> {
    let args: Vec<PathBuf> = crate::args_os().skip(1).map(PathBuf::from).collect();
    let [root, inputs, output] = args.as_slice() else {
        return Err("usage: tab_list_fixture RESOURCES INPUTS OUTPUT".into());
    };
    let atlas = HudAtlas::build(root)?;
    lang::set(lang::Language::load(&[root], "en_us"));
    let font = &atlas.font;
    let mut out = BufWriter::new(fs::File::create(output)?);
    for line in fs::read_to_string(inputs)?
        .lines()
        .filter(|l| !l.is_empty())
    {
        let c: Value = serde_json::from_str(line)?;
        let mut board = Scoreboard::default();
        for t in c["teams"].as_array().into_iter().flatten() {
            let name = t["name"].as_str().unwrap_or("");
            let team = board.add_team(name);
            team.color = t["color"].as_str().map(str::to_owned);
            team.prefix = t["prefix"].clone();
            team.suffix = t["suffix"].clone();
            for m in t["members"].as_array().into_iter().flatten() {
                board.add_to_team(m.as_str().unwrap_or(""), name);
            }
        }
        let objective_name = c["objective"]["name"].as_str().map(str::to_owned);
        if let Some(name) = &objective_name {
            let hearts = c["objective"]["renderType"] == "hearts";
            board.add_objective(
                name,
                "dummy",
                Value::String("x".into()),
                if hearts {
                    RenderType::Hearts
                } else {
                    RenderType::Integer
                },
                false,
                format(&c["objective"]["numberFormat"]),
            );
        }
        let uris: Vec<&str> = c["validUris"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut players = Vec::new();
        let mut upside = Vec::new();
        for p in c["players"].as_array().into_iter().flatten() {
            let uuid =
                u128::from_str_radix(&p["uuid"].as_str().unwrap_or("0").replace('-', ""), 16)?;
            let name = p["name"].as_str().unwrap_or("").to_owned();
            if p["inLevel"].as_bool().unwrap_or(false) && (name == "Dinnerbone" || name == "Grumm")
            {
                upside.push(uuid);
            }
            let mut player = TabPlayer::new(uuid, name);
            player.game_mode = p["gameMode"].as_i64().unwrap_or(0) as i32;
            player.latency = p["latency"].as_i64().unwrap_or(0) as i32;
            player.display_name = (!p["displayName"].is_null()).then(|| p["displayName"].clone());
            player.list_order = p["listOrder"].as_i64().unwrap_or(0) as i32;
            player.show_hat = p["showHat"].as_bool().unwrap_or(true);
            if let Some(t) = p.get("textures").filter(|t| !t.is_null()) {
                let signature = match t["signature"].as_str() {
                    None => skins::Signature::Unsigned,
                    Some("valid") => skins::Signature::Signed,
                    Some(_) => skins::Signature::Invalid,
                };
                let textures = skins::unpack(t["value"].as_str().unwrap_or(""), signature, &uris);
                let local = c["local"].as_str() == p["uuid"].as_str();
                let download = |_, url: &str| {
                    let hash = skins::texture_hash(url).unwrap_or_default();
                    match c["downloads"][&hash].as_str() {
                        Some("pending") => skins::Download::Pending,
                        Some("fail") => skins::Download::Failed,
                        _ => skins::Download::Ready,
                    }
                };
                if let Some(skin) = skins::lookup(&textures, !local, download) {
                    player.skin = skin;
                }
            }
            players.push(player);
        }
        let mut list = TabList::default();
        list.header = text(&c["header"]);
        list.footer = text(&c["footer"]);
        let background = tab_list::row_background(
            c["backgroundForChatOnly"].as_bool().unwrap_or(true),
            c["opacity"].as_f64().unwrap_or(0.5),
        );
        let mut frames = Vec::new();
        for f in c["frames"].as_array().into_iter().flatten() {
            if let Some(obj) = &objective_name {
                for (holder, v) in f["scores"].as_object().into_iter().flatten() {
                    let s = board.score_mut(holder, obj);
                    s.value = v["value"].as_i64().unwrap_or(0) as i32;
                    s.number_format = format(&v["format"]);
                }
                for r in f["reset"].as_array().into_iter().flatten() {
                    board.reset_one(r.as_str().unwrap_or(""), obj);
                }
            }
            let visible = f["visible"].as_bool().unwrap_or(false);
            list.set_visible(visible);
            let mut draws = Vec::new();
            if visible {
                let up = |id: u128| upside.contains(&id);
                let inputs = TabInputs {
                    players: &players,
                    board: &board,
                    objective: objective_name.as_deref().and_then(|n| board.objective(n)),
                    screen_width: c["width"].as_i64().unwrap_or(0) as i32,
                    online_mode: c["onlineMode"].as_bool().unwrap_or(false),
                    gui_ticks: f["tick"].as_i64().unwrap_or(0),
                    background,
                    upside_down: &up,
                    // Vanilla draws no mark.
                    mark: &|_| None,
                };
                for d in list.layout(&inputs, font) {
                    draws.push(match d {
                        GuiDraw::Fill { x0, y0, x1, y1, color } => {
                            json!({"type": "fill", "x0": x0, "y0": y0, "x1": x1, "y1": y1, "color": color as i32})
                        }
                        GuiDraw::Sprite { sprite, x, y, w, h, .. } => {
                            json!({"type": "sprite", "sprite": sprite, "x": x, "y": y, "w": w, "h": h})
                        }
                        GuiDraw::Text { text, x, y, color, shadow } => json!({
                            "type": "text", "runs": runs(&text), "x": x, "y": y, "color": color as i32, "shadow": shadow
                        }),
                        GuiDraw::Blit { texture, x, y, w, h, uv, src, tex, color, .. } => {
                            let a = [x as f64, y as f64, f64::from(uv[0]), f64::from(uv[1]), w as f64, h as f64,
                                src[0] as f64, src[1] as f64, tex[0] as f64, tex[1] as f64];
                            json!({"type": "blit", "texture": texture, "args": a, "color": color as i32})
                        }
                        other => json!({"type": "unexpected", "draw": format!("{other:?}")}),
                    });
                }
            }
            frames.push(Value::Array(draws));
        }
        writeln!(out, "{}", json!({ "frames": frames }))?;
    }
    Ok(())
}
