package dev.horizonlab.parity;

import com.google.gson.*;
import java.io.PrintWriter;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.util.debug.ServerDebugSubscribers;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.item.FallingBlockEntity;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;

/**
 * One `FallingBlockEntity.tick` on the block update oracle's level: the entity
 * is created on the block below the probe position, set on the ground and
 * ticked once, so it runs the falling entity's landing branch against the
 * case's carried state, the block at the position and its neighbours. Printed:
 * the landing decision (placed block, broken, or still falling on a moving
 * piston), the blocks the landing changed and the numeric level events.
 */
public final class VanillaFallingBlockOracle {
    /** The level's only chunk map caller: a landing placement sends packets. */
    static final class NoopChunkMap extends ChunkMap {
        NoopChunkMap() { super(null, null, null, null, null, null, null, null, null, null, 0, false); }
        @Override public void sendToTrackingPlayers(Entity entity, net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener> packet) { }
        @Override protected void sendToTrackingPlayersAndSelf(Entity entity, net.minecraft.network.protocol.Packet<? super net.minecraft.network.protocol.game.ClientGamePacketListener> packet) { }
    }

    static void prepare(VanillaBlockUpdateOracle.World w) throws Exception {
        w.entities = new ArrayList<>();
        w.registries = ParityData.access;
        w.type = VanillaBlockUpdateOracle.OVERWORLD_TYPE;
        w.dimensionKey = Level.OVERWORLD;
        VanillaBlockUpdateOracle.set(MinecraftServer.class, w.server, "debugSubscribers", new ServerDebugSubscribers(w.server));
        var unsafe = VanillaBlockUpdateOracle.unsafe;
        var noop = VanillaBlockUpdateOracle.allocate.invoke(unsafe, NoopChunkMap.class);
        var field = ServerChunkCache.class.getDeclaredField("chunkMap");
        long offset = (long) unsafe.getClass().getMethod("objectFieldOffset", java.lang.reflect.Field.class).invoke(unsafe, field);
        unsafe.getClass().getMethod("putObject", Object.class, long.class, Object.class).invoke(unsafe, w.getChunkSource(), offset, noop);
    }

    public static void main(String[] args) throws Exception {
        ParityData.init();
        Path resources = Path.of(args[0]);
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        VanillaHeightmapOracle.bindTags(JsonParser.parseString(Files.readString(resources.resolve("block-tags.json"))).getAsJsonObject());
        VanillaLevelShapeOracle.bindFluidTags(resources.resolve("data/minecraft/tags/fluid"));
        VanillaBlockUpdateOracle.OVERWORLD_TYPE = VanillaBlockUpdateOracle.loadOverworld();
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[2]))); var lines = Files.lines(Path.of(args[1]))) {
            int index = 0;
            for (var line : (Iterable<String>) lines::iterator) {
                var f = JsonParser.parseString(line).getAsJsonObject();
                out.println("== " + index++);
                try {
                    var copy = new JsonObject();
                    copy.add("cells", f.get("cells"));
                    copy.add("seed", f.get("seed"));
                    var w = VanillaBlockUpdateOracle.build(copy);
                    prepare(w);
                    BlockPos pos = VanillaBlockUpdateOracle.readPos(f.getAsJsonArray("pos"));
                    BlockState state = Block.stateById(f.get("state").getAsInt());
                    Map<BlockPos, BlockState> before = new HashMap<>(w.cells);
                    var entity = FallingBlockEntity.fall(w, pos, state);
                    // `fall` removes the block at the source cell; the falling block
                    // reached this cell from above, so put the destination back.
                    BlockState destination = before.getOrDefault(pos, VanillaBlockUpdateOracle.AIR);
                    if (!destination.isAir()) {
                        w.cells.put(pos, destination);
                    }
                    entity.time = f.get("time").getAsInt();
                    entity.setOnGround(true);
                    if (f.has("hurt")) {
                        entity.setHurtsEntities(f.get("hurt").getAsFloat(), f.get("hurt_max").getAsInt());
                    }
                    if (!f.get("drop").getAsBoolean()) {
                        entity.disableDrop();
                    }
                    entity.tick();
                    BlockState now = w.getBlockState(pos);
                    if (!entity.isRemoved()) {
                        out.println("decision continue");
                    } else if (!now.equals(before.getOrDefault(pos, VanillaBlockUpdateOracle.AIR))) {
                        out.println("decision placed " + Block.getId(now));
                    } else {
                        out.println("decision broken");
                    }
                    for (String s : VanillaBlockUpdateOracle.summary(w, before)) {
                        out.println(s);
                    }
                    for (String s : w.log.toString().split("\\n")) {
                        if (s.startsWith("L 1031 ")) {
                            out.println(s);
                        }
                    }
                } catch (Throwable e) {
                    Throwable c = e;
                    while (c.getCause() != null) c = c.getCause();
                    var st = c.getStackTrace();
                    out.println("error " + c.getClass().getSimpleName() + " " + (st.length > 0 ? st[0].toString() : "") + " " + c.getMessage());
                }
            }
        }
    }
}
