"""Prepare the declared local respawn observer using the existing official-codec client.

Exact replacement guards reject observer-source drift. This writes source, not classes.
"""
from pathlib import Path
import hashlib
import json


def prepare_client(root: Path, out: Path) -> Path:
    source = root/'mods/horizon/src/test/java/dev/horizonlab/parity/LocatorBarClient.java'
    out.mkdir(exist_ok=True)
    s = source.read_text()
    changes = []
    def replace(a, b):
        nonlocal s
        assert s.count(a) == 1, (a, s.count(a))
        changes.append({'from':a,'to':b})
        s = s.replace(a,b)
    
    replace('case "respawn" -> players.get(w[1]).send(new net.minecraft.network.protocol.game.ServerboundClientCommandPacket(\n                        net.minecraft.network.protocol.game.ServerboundClientCommandPacket.Action.PERFORM_RESPAWN));', '''case "respawn" -> {
                        Player p = players.get(w[1]);
                        p.spawnRecord("request-before", null);
                        p.send(new net.minecraft.network.protocol.game.ServerboundClientCommandPacket(
                            net.minecraft.network.protocol.game.ServerboundClientCommandPacket.Action.PERFORM_RESPAWN));
                        p.spawnRecord("request", null);
                        stdout.println("respawn-sent " + w[1]);
                    }
                    case "spawn-snapshot" -> {
                        players.get(w[1]).spawnRecord(w[2], null);
                        stdout.println("spawn-snapshotted " + w[1] + " " + w[2]);
                    }
                    case "spawn-loaded" -> {
                        Player p = players.get(w[1]);
                        p.send(new ServerboundPlayerLoadedPacket());
                        p.spawnRecord("loaded-ack", null);
                        stdout.println("spawn-loaded " + w[1]);
                    }''')
    replace('private volatile PositionMoveRotation position = new PositionMoveRotation(Vec3.ZERO, Vec3.ZERO, 0, 0);', '''private volatile PositionMoveRotation position = new PositionMoveRotation(Vec3.ZERO, Vec3.ZERO, 0, 0);
        private Float spawnHealth = null;
        private int spawnTeleports = 0;
        private int spawnLastTeleport = -1;
        private final java.util.Set<String> spawnChunks = java.util.concurrent.ConcurrentHashMap.newKeySet();

        private void spawnRecord(String kind, byte[] payload) {
            var j = new com.google.gson.JsonObject();
            j.addProperty("kind", kind);
            j.addProperty("profile_uuid", String.valueOf(profileUuid));
            j.addProperty("profile_name", profileName);
            j.addProperty("entity_id", id);
            var at = position.position();
            j.add("position", new com.google.gson.Gson().toJsonTree(new double[]{at.x,at.y,at.z}));
            j.addProperty("yaw_bits", Float.floatToRawIntBits(position.yRot()));
            j.addProperty("pitch_bits", Float.floatToRawIntBits(position.xRot()));
            j.addProperty("teleports", spawnTeleports);
            j.addProperty("teleport_id", spawnLastTeleport);
            if (spawnHealth != null) j.addProperty("health_bits", Float.floatToRawIntBits(spawnHealth));
            j.add("chunks", new com.google.gson.Gson().toJsonTree(new java.util.TreeSet<>(spawnChunks)));
            if (payload != null) j.addProperty("payload_hex", java.util.HexFormat.of().formatHex(payload));
            record("spawn-wire " + j);
        }''')
    replace('Packet<?> p = r.packet;\n                    if (p instanceof net.minecraft.network.protocol.game.ClientboundLoginPacket l)', '''Packet<?> p = r.packet;
                    if (p instanceof net.minecraft.network.protocol.game.ClientboundSetHealthPacket h) {
                        spawnHealth = h.getHealth();
                        spawnRecord("health", r.payload);
                    }
                    if (p instanceof net.minecraft.network.protocol.game.ClientboundRespawnPacket rs) {
                        spawnRecord("respawn", r.payload);
                        spawnChunks.clear();
                    }
                    if (p instanceof net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket chunk) {
                        spawnChunks.add(chunk.x() + "," + chunk.z());
                    }
                    if (p instanceof net.minecraft.network.protocol.game.ClientboundLoginPacket l)''')
    replace('id = l.playerId();', 'id = l.playerId();\n                        spawnRecord("login", r.payload);')
    replace('send(new ServerboundAcceptTeleportationPacket(t.id(), at.x, at.y, at.z, position.yRot(), position.xRot()));', '''send(new ServerboundAcceptTeleportationPacket(t.id(), at.x, at.y, at.z, position.yRot(), position.xRot()));
                        spawnTeleports++;
                        spawnLastTeleport = t.id();
                        spawnRecord("position-ack", r.payload);''')
    (out/'LocatorBarClient.java').write_text(s)
    (out/'adapter-manifest.json').write_text(json.dumps({'base_path':str(source),'base_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'derived_sha256':hashlib.sha256(s.encode()).hexdigest(),'changes':changes,'status':'UNCOMPILED_READ_ONLY_DRAFT','limitations':'Snapshot reflects actual received packet state only; server model separately admitted. No post-respawn MovePlayer packet added. Explicit loaded ack requires actual position acknowledgement and target chunk admission.'},indent=2)+'\n')
    return out/'LocatorBarClient.java'
