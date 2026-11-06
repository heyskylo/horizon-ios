package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import io.netty.buffer.Unpooled;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HexFormat;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundSoundEntityPacket;
import net.minecraft.network.protocol.game.ClientboundSoundPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.players.PlayerList;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.vehicle.minecart.Minecart;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;

/** The real ServerLevel sound overloads and packet codecs; only broadcast is a recording sink. */
public final class VanillaEntitySoundPacketOracle {
    static Object unsafe;
    static java.lang.reflect.Method allocate;
    static StubPlayers stubPlayers;
    static PrintWriter out;
    static <T> T make(Class<T> c) throws Exception { return c.cast(allocate.invoke(unsafe, c)); }
    static void set(Object o, Class<?> c, String name, Object value) throws Exception {
        var f = c.getDeclaredField(name); f.setAccessible(true); f.set(o, value);
    }
    static float f(String hex) { return Float.intBitsToFloat(Integer.parseUnsignedInt(hex, 16)); }
    static double d(String hex) { return Double.longBitsToDouble(Long.parseUnsignedLong(hex, 16)); }
    static final class StubPlayers extends PlayerList {
        StubPlayers() { super(null, null, null, (net.minecraft.server.notifications.NotificationService) null); }
        @Override public void broadcast(Player except, double x, double y, double z, double range,
                                        ResourceKey<Level> dimension, Packet<?> packet) {
            var bytes = new RegistryFriendlyByteBuf(Unpooled.buffer(),
                RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY));
            try {
                String kind;
                if (packet instanceof ClientboundSoundEntityPacket p) {
                    kind = "entity"; ClientboundSoundEntityPacket.STREAM_CODEC.encode(bytes, p);
                } else if (packet instanceof ClientboundSoundPacket p) {
                    kind = "position"; ClientboundSoundPacket.STREAM_CODEC.encode(bytes, p);
                } else { throw new AssertionError(packet); }
                byte[] payload = new byte[bytes.readableBytes()]; bytes.readBytes(payload);
                out.printf("%s %016x %s%n", kind, Double.doubleToRawLongBits(range), HexFormat.of().formatHex(payload));
            } finally { bytes.release(); }
        }
    }
    static final class StubServer extends MinecraftServer {
        StubServer() {
            super(null, null, null, null, null, null, null, null, null, false, null);
        }

        @Override public PlayerList getPlayerList() { return stubPlayers; }
        @Override public CommandSourceStack createCommandSourceStack() { return null; }
        @Override public void onTickRateChanged() { }
        @Override protected boolean initServer() { return false; }
        @Override public net.minecraft.server.permissions.LevelBasedPermissionSet operatorUserPermissions() { return null; }
        @Override public net.minecraft.server.permissions.PermissionSet getFunctionCompilationPermissions() { return null; }
        @Override public boolean shouldRconBroadcast() { return false; }
        @Override protected net.minecraft.util.debugchart.SampleLogger getTickTimeLogger() { return null; }
        @Override public boolean isTickTimeLoggingEnabled() { return false; }
        @Override public net.minecraft.SystemReport fillServerSystemReport(net.minecraft.SystemReport r) { return r; }
        @Override public boolean isDedicatedServer() { return false; }
        @Override public int getRateLimitPacketsPerSecond() { return 0; }
        @Override public int getCommandSpamThresholdSeconds() { return 0; }
        @Override public int getChatSpamThresholdSeconds() { return 0; }
        @Override public boolean useNativeTransport() { return false; }
        @Override public boolean isPublished() { return false; }
        @Override public int getMaxPlayers() { return 0; }
        @Override public boolean shouldInformAdmins() { return false; }
        @Override public boolean isSingleplayerOwner(net.minecraft.server.players.NameAndId id) { return false; }
    }


    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion(); net.minecraft.server.Bootstrap.bootStrap();
        if (args[0].equals("decode-recorded")) {
            // Existing Studio captures: the pinned codec, not a second packet implementation.
            try (var writer = new PrintWriter(Files.newBufferedWriter(Path.of(args[2])));
                 var lines = Files.lines(Path.of(args[1]))) {
                for (var line : (Iterable<String>) lines::iterator) {
                    var row = JsonParser.parseString(line).getAsJsonObject();
                    byte[] payload = HexFormat.of().parseHex(row.get("payload").getAsString());
                    var access = RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
                    var bytes = new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(payload), access);
                    var again = new RegistryFriendlyByteBuf(Unpooled.buffer(), access);
                    try {
                        int id = bytes.readVarInt();
                        if (id != 119) throw new AssertionError("not a pinned positional sound: " + id);
                        var p = ClientboundSoundPacket.STREAM_CODEC.decode(bytes);
                        if (bytes.isReadable()) throw new AssertionError("trailing sound bytes");
                        again.writeVarInt(id);
                        ClientboundSoundPacket.STREAM_CODEC.encode(again, p);
                        byte[] encoded = new byte[again.readableBytes()]; again.readBytes(encoded);
                        if (!java.util.Arrays.equals(payload, encoded)) throw new AssertionError("sound roundtrip");
                        row.addProperty("event", p.getSound().value().location().toString());
                        row.addProperty("category", p.getSource().ordinal());
                        row.addProperty("x", p.getX()); row.addProperty("y", p.getY()); row.addProperty("z", p.getZ());
                        row.addProperty("volume_bits", Integer.toHexString(Float.floatToRawIntBits(p.getVolume())));
                        row.addProperty("pitch_bits", Integer.toHexString(Float.floatToRawIntBits(p.getPitch())));
                        row.addProperty("seed", p.getSeed());
                        writer.println(row);
                    } finally { bytes.release(); again.release(); }
                }
            }
            return;
        }
        var u = Class.forName("sun.misc.Unsafe"); var field = u.getDeclaredField("theUnsafe");
        field.setAccessible(true); unsafe = field.get(null); allocate = u.getMethod("allocateInstance", Class.class);
        stubPlayers = make(StubPlayers.class); var server = make(StubServer.class); var level = make(ServerLevel.class);
        set(level, ServerLevel.class, "server", server); set(level, Level.class, "dimension", Level.OVERWORLD);
        var emitter = make(Minecart.class);
        try (var writer = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])));
             var lines = Files.lines(Path.of(args[0]))) {
            out = writer;
            for (var line : (Iterable<String>) lines::iterator) {
                var c = JsonParser.parseString(line).getAsJsonObject();
                int id = c.get("entity").getAsInt(); set(emitter, Entity.class, "id", id);
                var a = c.getAsJsonArray("position");
                double x = d(a.get(0).getAsString()), y = d(a.get(1).getAsString()), z = d(a.get(2).getAsString());
                set(emitter, Entity.class, "position", new Vec3(x, y, z));
                Holder<SoundEvent> event;
                if (c.get("sound_id").isJsonNull()) {
                    event = Holder.direct(SoundEvent.createVariableRangeEvent(Identifier.parse("fixture:bound")));
                } else { event = BuiltInRegistries.SOUND_EVENT.get(c.get("sound_id").getAsInt()).orElseThrow(); }
                if (event.value().fixedRange().isPresent()) { throw new AssertionError("fixture requires a variable-range event"); }
                var source = SoundSource.values()[c.get("source").getAsInt()];
                float volume = f(c.get("volume").getAsString()), pitch = f(c.get("pitch").getAsString());
                long seed = c.get("seed").getAsLong();
                if (c.get("bound").getAsBoolean()) {
                    level.playSeededSound(null, emitter, event, source, volume, pitch, seed);
                } else { level.playSeededSound(null, x, y, z, event, source, volume, pitch, seed); }
            }
        }
        System.exit(0);
    }
}
