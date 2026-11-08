//! The component fields of protocol 764, before 765 changed their wire codec to NBT.
//! Other NBT and item bytes retain their old layout for the following translation steps.
use crate::{
    Error, Result, State,
    adapter::Session,
    buf::{Codec, Reader, Writer},
    ids,
    packets::play,
};
use horizon_world::{datafix::json, nbt::Tag};

pub(crate) fn read_component(r: &mut Reader) -> Result<Tag> {
    let value = r.string(262144)?;
    let mut value = json::parse_prefix_strict(&value.encode_utf16().collect::<Vec<_>>())
        .ok_or_else(|| Error::new("invalid legacy component JSON"))?;
    if matches!(&value, json::Json::Arr(values) if values.is_empty()) {
        return Err(Error::new("empty legacy component list"));
    }
    component_json(&mut value)?;
    json::to_nbt(&value).ok_or_else(|| Error::new("null legacy component"))
}

fn key(key: &[u16], name: &str) -> bool {
    key.iter().copied().eq(name.encode_utf16())
}

/// The old Gson codec writes an unstyled literal as {"text":"..."}; the 765 codec
/// uses a string. Hover-item counts keep their value types; primitive translation
/// arguments become strings like the old Gson serializer, retaining their spelling.
fn component_json(value: &mut json::Json) -> Result<()> {
    use json::Json;
    match value {
        Json::Num(v) => *value = Json::string(v),
        Json::Bool(v) => *value = Json::string(if *v { "true" } else { "false" }),
        Json::Arr(values) => {
            for value in values {
                component_json(value)?;
            }
        }
        Json::Obj(values) => {
            if !values.iter().any(|(name, _)| {
                ["text", "translate", "score", "selector", "keybind", "nbt"]
                    .iter()
                    .any(|content| key(name, content))
            }) {
                return Err(Error::new("missing legacy component content"));
            }
            for (name, child) in values.iter_mut() {
                if key(name, "extra") || key(name, "separator") {
                    component_json(child)?;
                } else if key(name, "with") {
                    if let Json::Arr(args) = child {
                        for arg in args {
                            if matches!(arg, Json::Null) {
                                return Err(Error::new("null legacy translation argument"));
                            }
                            component_json(arg)?;
                        }
                    }
                } else if key(name, "text") {
                    if !matches!(child, Json::Str(_) | Json::Num(_) | Json::Bool(_)) {
                        return Err(Error::new("invalid legacy literal text"));
                    }
                    component_json(child)?;
                } else if [
                    "bold",
                    "italic",
                    "underlined",
                    "strikethrough",
                    "obfuscated",
                ]
                .iter()
                .any(|flag| key(name, flag))
                {
                    let flag = match child {
                        Json::Bool(value) => *value,
                        Json::Str(value) => {
                            String::from_utf16_lossy(value).eq_ignore_ascii_case("true")
                        }
                        Json::Num(_) => false,
                        _ => return Err(Error::new("invalid legacy style flag")),
                    };
                    *child = Json::Bool(flag);
                } else if key(name, "hoverEvent") {
                    let action = child.get("action").and_then(|v| match v {
                        Json::Str(s) => Some(String::from_utf16_lossy(s)),
                        _ => None,
                    });
                    if let Json::Obj(fields) = child {
                        for (name, content) in fields {
                            if !key(name, "contents") && !key(name, "value") {
                                continue;
                            }
                            match action.as_deref() {
                                Some("show_text") => component_json(content)?,
                                Some("show_entity") => {
                                    if let Json::Obj(entity) = content {
                                        for (name, component) in entity {
                                            if key(name, "name") {
                                                component_json(component)?;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if values.len() == 1
                && key(&values[0].0, "text")
                && matches!(&values[0].1, Json::Str(_))
            {
                *value = std::mem::replace(&mut values[0].1, Json::Null);
            }
        }
        Json::Null => return Err(Error::new("null legacy component")),
        _ => {}
    }
    Ok(())
}

/// Copy primitive spans exactly, replacing only explicitly identified component fields.
struct Fields<'a> {
    r: Reader<'a>,
    w: Writer,
}
impl<'a> Fields<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self {
            r: Reader::new(payload),
            w: Writer::new(),
        }
    }
    fn copy<T>(&mut self, read: impl FnOnce(&mut Reader<'a>) -> Result<T>) -> Result<T> {
        let start = self.r.position();
        let value = read(&mut self.r)?;
        self.w.bytes(self.r.since(start));
        Ok(value)
    }
    fn int(&mut self) -> Result<i32> {
        self.copy(Reader::varint)
    }
    fn byte(&mut self) -> Result<u8> {
        self.copy(Reader::u8)
    }
    fn flag(&mut self) -> Result<bool> {
        self.copy(Reader::bool)
    }
    fn string(&mut self) -> Result<String> {
        self.copy(|r| r.string(32767))
    }
    fn count(&mut self) -> Result<usize> {
        self.copy(|r| r.count(65536))
    }
    fn bytes(&mut self, n: usize) -> Result<()> {
        self.copy(|r| r.bytes(n).map(|_| ()))
    }
    fn byte_array(&mut self) -> Result<()> {
        self.copy(|r| {
            let n = r.count(r.remaining())?;
            r.bytes(n)?;
            Ok(())
        })
    }
    fn component(&mut self) -> Result<()> {
        self.w.nbt(&read_component(&mut self.r)?);
        Ok(())
    }
    fn optional_component(&mut self) -> Result<()> {
        if self.flag()? {
            self.component()?;
        }
        Ok(())
    }
    fn optional_string(&mut self) -> Result<()> {
        if self.flag()? {
            self.string()?;
        }
        Ok(())
    }
    fn strings(&mut self) -> Result<()> {
        for _ in 0..self.count()? {
            self.string()?;
        }
        Ok(())
    }
    fn bound_chat_type(&mut self) -> Result<()> {
        self.int()?;
        self.component()?;
        self.optional_component()
    }
    fn resource_pack(&mut self) -> Result<()> {
        self.string()?;
        self.copy(|r| r.string(40))?;
        self.flag()?;
        self.optional_component()
    }
    fn player_chat(&mut self) -> Result<()> {
        self.bytes(16)?;
        self.int()?;
        if self.flag()? {
            self.bytes(256)?;
        }
        self.copy(|r| r.string(256))?;
        self.bytes(16)?; // timestamp and salt
        for _ in 0..self.count()? {
            match self.int()? {
                0 => self.bytes(256)?,
                id if id > 0 => {}
                _ => return Err(Error::new("negative legacy signature reference")),
            }
        }
        self.optional_component()?;
        self.copy(play::FilterMask::read)?;
        self.bound_chat_type()
    }
    fn player_info(&mut self) -> Result<()> {
        let actions = self.byte()?;
        if actions & !63 != 0 {
            return Err(Error::new("invalid legacy player-info actions"));
        }
        for _ in 0..self.count()? {
            self.bytes(16)?;
            if actions & 1 != 0 {
                self.copy(|r| r.string(16))?;
                for _ in 0..self.count()? {
                    self.string()?;
                    self.string()?;
                    self.optional_string()?;
                }
            }
            if actions & 2 != 0 && self.flag()? {
                self.bytes(24)?; // chat-session UUID and key expiry
                self.byte_array()?;
                self.byte_array()?;
            }
            if actions & 4 != 0 {
                self.int()?;
            }
            if actions & 8 != 0 {
                self.flag()?;
            }
            if actions & 16 != 0 {
                self.int()?;
            }
            if actions & 32 != 0 {
                self.optional_component()?;
            }
        }
        Ok(())
    }
    fn map(&mut self) -> Result<()> {
        self.int()?;
        self.byte()?;
        self.flag()?;
        if self.flag()? {
            for _ in 0..self.count()? {
                self.int()?;
                self.bytes(3)?;
                self.optional_component()?;
            }
        }
        if self.byte()? != 0 {
            self.bytes(3)?;
            self.byte_array()?;
        }
        Ok(())
    }
    fn team(&mut self) -> Result<()> {
        self.string()?;
        let method = self.byte()?;
        if method > 4 {
            return Err(Error::new("invalid legacy team method"));
        }
        if matches!(method, 0 | 2) {
            self.component()?;
            self.byte()?;
            self.string()?;
            self.string()?;
            self.int()?;
            self.component()?;
            self.component()?;
        }
        if matches!(method, 0 | 3 | 4) {
            self.strings()?;
        }
        Ok(())
    }
    fn advancements(&mut self) -> Result<()> {
        self.flag()?;
        for _ in 0..self.count()? {
            self.string()?;
            self.optional_string()?;
            if self.flag()? {
                self.component()?;
                self.component()?;
                // An old item is present + id + signed-byte count + unnamed NBT. Copy it
                // raw: the later shared reader upgrades and renumbers it exactly once.
                self.copy(|r| {
                    if r.bool()? {
                        r.varint()?;
                        r.i8()?;
                        r.nbt()?;
                    }
                    Ok(())
                })?;
                self.int()?;
                let flags = self.copy(Reader::i32)?;
                if flags & 1 != 0 {
                    self.string()?;
                }
                self.bytes(8)?; // the old display's x/y position
            }
            for _ in 0..self.count()? {
                self.strings()?;
            }
            self.flag()?; // sends telemetry
        }
        self.strings()?;
        for _ in 0..self.count()? {
            self.string()?;
            for _ in 0..self.count()? {
                self.string()?;
                if self.flag()? {
                    self.bytes(8)?;
                }
            }
        }
        Ok(())
    }
    fn finish(self) -> Result<Vec<u8>> {
        if self.r.remaining() != 0 {
            return Err(Error::new("trailing legacy text packet bytes"));
        }
        Ok(self.w.data)
    }
}

/// `id` is already the pinned packet id; the body is still protocol 764.
pub(crate) fn clientbound(
    _session: &Session,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<Option<Vec<u8>>> {
    let mut f = Fields::new(payload);
    if state == State::Configuration {
        match id {
            ids::configuration::clientbound::DISCONNECT => f.component()?,
            ids::configuration::clientbound::RESOURCE_PACK_PUSH => f.resource_pack()?,
            _ => return Ok(None),
        }
    } else if state == State::Play {
        use ids::play::clientbound as cb;
        match id {
            cb::DISCONNECT
            | cb::SET_ACTION_BAR_TEXT
            | cb::SET_SUBTITLE_TEXT
            | cb::SET_TITLE_TEXT => f.component()?,
            cb::SYSTEM_CHAT => {
                f.component()?;
                f.flag()?;
            }
            cb::TAB_LIST => {
                f.component()?;
                f.component()?;
            }
            cb::RESOURCE_PACK_PUSH => f.resource_pack()?,
            cb::SERVER_DATA => {
                f.component()?;
                if f.flag()? {
                    f.byte_array()?;
                }
                f.flag()?;
            }
            cb::OPEN_SCREEN => {
                f.int()?;
                f.int()?;
                f.component()?;
            }
            cb::PLAYER_COMBAT_KILL => {
                f.int()?;
                f.component()?;
            }
            cb::BOSS_EVENT => {
                f.bytes(16)?;
                match f.int()? {
                    0 => {
                        f.component()?;
                        f.bytes(4)?;
                        f.int()?;
                        f.int()?;
                        f.byte()?;
                    }
                    1 => {}
                    2 => f.bytes(4)?,
                    3 => f.component()?,
                    4 => {
                        f.int()?;
                        f.int()?;
                    }
                    5 => {
                        f.byte()?;
                    }
                    _ => return Err(Error::new("invalid legacy boss operation")),
                }
            }
            cb::COMMAND_SUGGESTIONS => {
                f.int()?;
                f.int()?;
                f.int()?;
                for _ in 0..f.count()? {
                    f.string()?;
                    f.optional_component()?;
                }
            }
            cb::DISGUISED_CHAT => {
                f.component()?;
                f.bound_chat_type()?;
            }
            cb::PLAYER_CHAT => f.player_chat()?,
            cb::PLAYER_INFO_UPDATE => f.player_info()?,
            cb::MAP_ITEM_DATA => f.map()?,
            cb::SET_OBJECTIVE => {
                f.string()?;
                match f.byte()? {
                    0 | 2 => {
                        f.component()?;
                        f.int()?;
                    }
                    1 => {}
                    _ => return Err(Error::new("invalid legacy objective method")),
                }
            }
            cb::SET_PLAYER_TEAM => f.team()?,
            cb::UPDATE_ADVANCEMENTS => f.advancements()?,
            _ => return Ok(None),
        }
    } else {
        // Login-disconnect deliberately retains its JSON string in 765, too.
        return Ok(None);
    }
    f.finish().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ServerVersion, ids as context};

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn rich_components_match_nbt_independently_accepted_by_the_vanilla_765_reader() {
        // Real JsonOps/NbtOps bytes decoded by vanilla readComponentTrusted; Component.equals
        // independently confirms mixed lists, style booleans and both structured hover forms.
        for (name, json, nbt) in [
            (
                "literal",
                "\"Horizon wire\"",
                "08000c486f72697a6f6e2077697265",
            ),
            (
                "list",
                "[\"First\",{\"text\":\"Second\",\"bold\":true},{\"text\":\"Third\",\"italic\":false}]",
                "090a0000000308000000054669727374000800047465787400065365636f6e64010004626f6c64010008000474657874000554686972640100066974616c69630000",
            ),
            (
                "style",
                "{\"text\":\"Root\",\"bold\":true,\"italic\":false,\"color\":\"gold\",\"extra\":[\" plain\",{\"translate\":\"chat.type.text\",\"with\":[{\"text\":\"Wire\",\"bold\":false},\"hello\"]}]}",
                "0a080005636f6c6f720004676f6c6409000565787472610a00000002080000000620706c61696e00090004776974680a0000000208000474657874000457697265010004626f6c640000080000000568656c6c6f000800097472616e736c617465000e636861742e747970652e7465787400080004746578740004526f6f74010004626f6c64010100066974616c69630000",
            ),
            (
                "hover_item",
                "{\"text\":\"Item\",\"hoverEvent\":{\"action\":\"show_item\",\"contents\":{\"id\":\"minecraft:stone\",\"count\":2,\"tag\":\"{HorizonWire:1b}\"}}}",
                "0a0a000a686f7665724576656e740a0008636f6e74656e7473010005636f756e74020800026964000f6d696e6563726166743a73746f6e6508000374616700107b486f72697a6f6e576972653a31627d00080006616374696f6e000973686f775f6974656d000800047465787400044974656d00",
            ),
            (
                "hover_entity",
                "{\"text\":\"Entity\",\"hoverEvent\":{\"action\":\"show_entity\",\"contents\":{\"type\":\"minecraft:pig\",\"id\":\"00000000-0000-0000-0000-000000000001\",\"name\":{\"text\":\"Wire entity\",\"bold\":true}}}}",
                "0a0a000a686f7665724576656e740a0008636f6e74656e74730a00046e616d6508000474657874000b5769726520656e74697479010004626f6c6401000800026964002430303030303030302d303030302d303030302d303030302d30303030303030303030303108000474797065000d6d696e6563726166743a70696700080006616374696f6e000b73686f775f656e7469747900080004746578740006456e7469747900",
            ),
        ] {
            let mut old = Writer::new();
            old.string(json);
            old.u8(99);
            let mut r = Reader::new(&old.data);
            let actual = read_component(&mut r).unwrap();
            let expected = Reader::new(&bytes(nbt)).nbt().unwrap();
            assert_eq!(actual, expected, "{name}");
            assert_eq!(r.u8().unwrap(), 99, "{name}");
        }
    }

    #[test]
    fn every_text_packet_family_matches_actual_paired_packet_codecs() {
        use ids::play::clientbound as cb;
        let session = Session::new(ServerVersion::V1_20_2);
        // Actual vanilla packet constructors read these synthetic payloads, consume every byte,
        // then the real packet writers encode them independently on 764 and 765.
        for (name, id, old, newer) in [
            (
                "disconnect",
                cb::DISCONNECT,
                "157b2274657874223a22646973636f6e6e656374227d",
                "08000a646973636f6e6e656374",
            ),
            (
                "actionbar",
                cb::SET_ACTION_BAR_TEXT,
                "147b2274657874223a22616374696f6e626172227d",
                "080009616374696f6e626172",
            ),
            (
                "title",
                cb::SET_TITLE_TEXT,
                "107b2274657874223a227469746c65227d",
                "0800057469746c65",
            ),
            (
                "subtitle",
                cb::SET_SUBTITLE_TEXT,
                "137b2274657874223a227375627469746c65227d",
                "0800087375627469746c65",
            ),
            (
                "system",
                cb::SYSTEM_CHAT,
                "117b2274657874223a2273797374656d227d01",
                "08000673797374656d01",
            ),
            (
                "tab",
                cb::TAB_LIST,
                "117b2274657874223a22686561646572227d117b2274657874223a22666f6f746572227d",
                "080006686561646572080006666f6f746572",
            ),
            (
                "boss_add",
                cb::BOSS_EVENT,
                "00000000000000000000000000000001000f7b2274657874223a22626f7373227d3f000000020305",
                "0000000000000000000000000000000100080004626f73733f000000020305",
            ),
            (
                "boss_name",
                cb::BOSS_EVENT,
                "0000000000000000000000000000000103127b2274657874223a2272656e616d6564227d",
                "000000000000000000000000000000010308000772656e616d6564",
            ),
            (
                "suggestions",
                cb::COMMAND_SUGGESTIONS,
                "2902030205666972737401127b2274657874223a22746f6f6c746970227d067365636f6e6400",
                "2902030205666972737401080007746f6f6c746970067365636f6e6400",
            ),
            (
                "disguised",
                cb::DISGUISED_CHAT,
                "127b2274657874223a226d657373616765227d01117b2274657874223a2273656e646572227d01117b2274657874223a22746172676574227d",
                "0800076d6573736167650108000673656e64657201080006746172676574",
            ),
            (
                "map",
                cb::MAP_ITEM_DATA,
                "11020101020201fe0701147b2274657874223a226d6170206c6162656c227d030304080001010203012a",
                "11020101020201fe07010800096d6170206c6162656c030304080001010203012a",
            ),
            (
                "open",
                cb::OPEN_SCREEN,
                "05030f7b2274657874223a226d656e75227d",
                "05030800046d656e75",
            ),
            (
                "combat",
                cb::PLAYER_COMBAT_KILL,
                "2a107b2274657874223a226465617468227d",
                "2a0800056465617468",
            ),
            (
                "player_chat",
                cb::PLAYER_CHAT,
                "000000000000000000000000000000010201000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff105369676e6564207769726520626f647900000000000004d2000000000000162e020100000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff01137b2274657874223a22756e7369676e6564227d0201000000000000000500117b2274657874223a2273656e646572227d01117b2274657874223a22746172676574227d",
                "000000000000000000000000000000010201000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff105369676e6564207769726520626f647900000000000004d2000000000000162e020100000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff01080008756e7369676e6564020100000000000000050008000673656e64657201080006746172676574",
            ),
            (
                "player_chat_plain",
                cb::PLAYER_CHAT,
                "0000000000000000000000000000000203000a506c61696e20626f64790000000000000000000000000000000000000002117b2274657874223a2273656e646572227d00",
                "0000000000000000000000000000000203000a506c61696e20626f6479000000000000000000000000000000000000000208000673656e64657200",
            ),
            (
                "player_info",
                cb::PLAYER_INFO_UPDATE,
                "3f01000000000000000000000000000000010b486f72697a6f6e57697265010874657874757265730973796e746865746963000001012a01127b2274657874223a22646973706c6179227d",
                "3f01000000000000000000000000000000010b486f72697a6f6e57697265010874657874757265730973796e746865746963000001012a01080007646973706c6179",
            ),
            (
                "player_info_session",
                cb::PLAYER_INFO_UPDATE,
                "220100000000000000000000000000000002010000000000000000000000000000000300000000000004d2a60230820122300d06092a864886f70d01010105000382010f003082010a0282010100d789f8cca32bffe5d2bb20630827fcb1c71e5f2d1f31decc702867e02b55a846f1fe5fb94357794970feb1861e1a45c46a7800b02d91c8fcd3f6a0a7b7f16c6da9796263ae4c38b423df331f81b4382e0b6aa5572b360d121d72c6541d360774f4bcb0040c698f2d8bc5ed8b933324962eb482bf3fb67b693395aeca5797f0e338173d56e0b018557da35e8bc4b5c356ecfd048d83f7fb61175f3763f6b1520c577e2dd4aa5e3c0f9d4c6ee8032a27fe79ab4b35b6a5ded0f0a79a2b9246c5786792695cb4395452259ec83c0435decef3d3e0a17ab1d11e3ebd692317c869a1566f73c566393101534f64af4d7abfd673cbcd6f8c1e10f104e4317e3afaf72b02030100010301020300",
                "220100000000000000000000000000000002010000000000000000000000000000000300000000000004d2a60230820122300d06092a864886f70d01010105000382010f003082010a0282010100d789f8cca32bffe5d2bb20630827fcb1c71e5f2d1f31decc702867e02b55a846f1fe5fb94357794970feb1861e1a45c46a7800b02d91c8fcd3f6a0a7b7f16c6da9796263ae4c38b423df331f81b4382e0b6aa5572b360d121d72c6541d360774f4bcb0040c698f2d8bc5ed8b933324962eb482bf3fb67b693395aeca5797f0e338173d56e0b018557da35e8bc4b5c356ecfd048d83f7fb61175f3763f6b1520c577e2dd4aa5e3c0f9d4c6ee8032a27fe79ab4b35b6a5ded0f0a79a2b9246c5786792695cb4395452259ec83c0435decef3d3e0a17ab1d11e3ebd692317c869a1566f73c566393101534f64af4d7abfd673cbcd6f8c1e10f104e4317e3afaf72b02030100010301020300",
            ),
            (
                "server_data",
                cb::SERVER_DATA,
                "0f7b2274657874223a226d6f7464227d010301020301",
                "0800046d6f7464010301020301",
            ),
            (
                "objective0",
                cb::SET_OBJECTIVE,
                "047769726500147b2274657874223a226f626a656374697665227d01",
                "0477697265000800096f626a65637469766501",
            ),
            (
                "objective2",
                cb::SET_OBJECTIVE,
                "047769726502147b2274657874223a226f626a656374697665227d01",
                "0477697265020800096f626a65637469766501",
            ),
            (
                "team",
                cb::SET_PLAYER_TEAM,
                "0477697265000f7b2274657874223a227465616d227d0306616c776179730e707573684f746865725465616d7304117b2274657874223a22707265666978227d117b2274657874223a22737566666978227d02055769726541055769726542",
                "0477697265000800047465616d0306616c776179730e707573684f746865725465616d730408000670726566697808000673756666697802055769726541055769726542",
            ),
            (
                "advancements",
                cb::UPDATE_ADVANCEMENTS,
                "000108776972653a6164760109776972653a726f6f7401147b2274657874223a22616476207469746c65227d1a7b2274657874223a22616476206465736372697074696f6e227d0101020a0100045769726501000100000007396d696e6563726166743a74657874757265732f6775692f616476616e63656d656e74732f6261636b67726f756e64732f73746f6e652e706e673fa00000c02000000201036f6e65010374776f01010c776972653a72656d6f7665640108776972653a616476020374776f00036f6e650100000000000003e8",
                "000108776972653a6164760109776972653a726f6f7401080009616476207469746c6508000f616476206465736372697074696f6e0101020a0100045769726501000100000007396d696e6563726166743a74657874757265732f6775692f616476616e63656d656e74732f6261636b67726f756e64732f73746f6e652e706e673fa00000c02000000201036f6e65010374776f01010c776972653a72656d6f7665640108776972653a616476020374776f00036f6e650100000000000003e8",
            ),
            (
                "resource",
                cb::RESOURCE_PACK_PUSH,
                "2068747470733a2f2f6578616d706c652e696e76616c69642f7061636b2e7a697028303030303030303030303030303030303030303030303030303030303030303030303030303030300101167b2274657874223a227061636b2070726f6d7074227d",
                "2068747470733a2f2f6578616d706c652e696e76616c69642f7061636b2e7a69702830303030303030303030303030303030303030303030303030303030303030303030303030303030010108000b7061636b2070726f6d7074",
            ),
        ] {
            let old = bytes(old);
            assert_eq!(
                clientbound(&session, State::Play, id, &old)
                    .unwrap()
                    .unwrap(),
                bytes(newer),
                "{name}"
            );
            assert!(
                clientbound(&session, State::Play, id, &old[..old.len() - 1]).is_err(),
                "{name}"
            );
            let mut trailing = old;
            trailing.push(99);
            assert!(
                clientbound(&session, State::Play, id, &trailing).is_err(),
                "{name}"
            );
            // The two shared common packets have the same body in configuration.
            let configuration_id = match name {
                "disconnect" => Some(ids::configuration::clientbound::DISCONNECT),
                "resource" => Some(ids::configuration::clientbound::RESOURCE_PACK_PUSH),
                _ => None,
            };
            if let Some(id) = configuration_id {
                assert_eq!(
                    clientbound(
                        &session,
                        State::Configuration,
                        id,
                        &trailing[..trailing.len() - 1]
                    )
                    .unwrap()
                    .unwrap(),
                    bytes(newer)
                );
            }
        }
    }

    #[test]
    fn metadata_text_uses_the_field_codec_and_login_stays_json() {
        use crate::entity_data::{EntityData, Value, serializer};
        let session = Session::new(ServerVersion::V1_20_2);
        for kind in [serializer::COMPONENT, serializer::OPTIONAL_COMPONENT] {
            let mut old = Writer::new();
            old.u8(2);
            old.varint(kind);
            if kind == serializer::OPTIONAL_COMPONENT {
                old.bool(true);
            }
            old.string(r#"{"text":"Metadata"}"#);
            old.u8(255);
            old.u8(99);
            let mut r = Reader::new(&old.data);
            let entries = {
                let _up = context::renumbering(session.version.ids().unwrap(), context::Way::Up);
                EntityData::read(&mut r).unwrap()
            };
            let text = Tag::String("Metadata".into());
            assert_eq!(
                entries,
                vec![(
                    2,
                    kind,
                    if kind == serializer::COMPONENT {
                        Value::Text(text)
                    } else {
                        Value::OptText(Some(text))
                    }
                )]
            );
            assert_eq!(r.u8().unwrap(), 99);
        }
        let mut login = Writer::new();
        login.string(r#"{"text":"Login remains JSON"}"#);
        assert!(
            clientbound(
                &session,
                State::Login,
                ids::login::clientbound::LOGIN_DISCONNECT,
                &login.data
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn component_syntax_matches_the_vanilla_gson_reader_boundary() {
        let parse = |json: &str| {
            let mut w = Writer::new();
            w.string(json);
            read_component(&mut Reader::new(&w.data))
        };
        // Actual 764 FriendlyByteBuf.readComponent accepts one strict value and ignores
        // following input. It coerces primitive literals and Style's primitive booleans.
        for (json, expected) in [
            (r#""x" junk"#, "x"),
            (r#"{"text":"x"}junk"#, "x"),
            ("1 2", "1"),
            ("42", "42"),
            ("true", "true"),
            ("TRUE", "true"),
            ("true:", "true"),
            ("true]", "true"),
        ] {
            assert_eq!(parse(json).unwrap(), Tag::String(expected.into()), "{json}");
        }
        for (value, expected) in [
            (r#""true""#, 1),
            (r#""TRUE""#, 1),
            (r#""false""#, 0),
            (r#""anything""#, 0),
            ("1", 0),
        ] {
            let tag = parse(&format!(r#"{{"text":"x","bold":{value}}}"#)).unwrap();
            assert_eq!(tag.get("bold"), Some(&Tag::Byte(expected)), "{value}");
        }
        for json in [
            r#"{text:'x'}"#,
            r#"/* c */{"text":"x"}"#,
            r#"["a",]"#,
            r#"{"text":"x",}"#,
            "truejunk",
            "falsex",
            "nullx",
            "12junk",
            "1.2x",
            "true/comment",
            "null",
            "[]",
            "{}junk",
            "[]junk",
            r#"{"text":"x","bold":null}"#,
            r#"{"translate":"wire.key","with":[null]}"#,
        ] {
            assert!(parse(json).is_err(), "{json}");
        }
    }

    #[test]
    fn advancement_item_nbt_is_copied_without_text_conversion() {
        let session = Session::new(ServerVersion::V1_20_2);
        let opaque = Tag::compound([("json", Tag::String(r#"{"text":"Opaque item tag"}"#.into()))]);
        let mut old = Writer::new();
        old.bool(false);
        old.varint(1);
        old.string("wire:opaque");
        old.bool(false);
        old.bool(true);
        old.string(r#"{"text":"Title"}"#);
        old.string(r#"{"text":"Description"}"#);
        old.bool(true);
        old.varint(1);
        old.i8(2);
        old.nbt(&opaque);
        old.varint(0);
        old.i32(0);
        old.f32(0.0);
        old.f32(0.0);
        old.varint(0);
        old.bool(false);
        old.varint(0);
        old.varint(0);
        let body = clientbound(
            &session,
            State::Play,
            ids::play::clientbound::UPDATE_ADVANCEMENTS,
            &old.data,
        )
        .unwrap()
        .unwrap();
        let mut r = Reader::new(&body);
        assert!(!r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string(32767).unwrap(), "wire:opaque");
        assert!(!r.bool().unwrap());
        assert!(r.bool().unwrap());
        assert_eq!(r.nbt().unwrap(), Tag::String("Title".into()));
        assert_eq!(r.nbt().unwrap(), Tag::String("Description".into()));
        assert!(r.bool().unwrap());
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.i8().unwrap(), 2);
        assert_eq!(r.nbt().unwrap(), opaque);
    }

    #[test]
    fn primitive_translation_arguments_keep_the_764_rendered_spellings() {
        // The real 764 writer turns primitive args into strings. Actual 765 writer/readback
        // of those strings preserves 764 getString (including true, 1.0, 1e2 and -0.0).
        for (name, json, nbt) in [
            (
                "mixed",
                "{\"translate\":\"wire.key\",\"fallback\":\"Args %s|%s|%s\",\"with\":[true,1.0,1e2]}",
                "0a0900047769746808000000030004747275650003312e30000331653208000866616c6c6261636b000d417267732025737c25737c25730800097472616e736c6174650008776972652e6b657900",
            ),
            (
                "numeric",
                "{\"translate\":\"wire.key\",\"fallback\":\"Args %s|%s|%s|%s\",\"with\":[1,1.0,1e2,-0.0]}",
                "0a0900047769746808000000040001310003312e30000331653200042d302e3008000866616c6c6261636b0010417267732025737c25737c25737c25730800097472616e736c6174650008776972652e6b657900",
            ),
            (
                "booleans",
                "{\"translate\":\"wire.key\",\"fallback\":\"Args %s|%s\",\"with\":[true,false]}",
                "0a090004776974680800000002000474727565000566616c736508000866616c6c6261636b000a417267732025737c25730800097472616e736c6174650008776972652e6b657900",
            ),
        ] {
            let mut wire = Writer::new();
            wire.string(json);
            let actual = read_component(&mut Reader::new(&wire.data)).unwrap();
            let expected = Reader::new(&bytes(nbt)).nbt().unwrap();
            assert_eq!(actual, expected, "{name}");
        }
    }
}
