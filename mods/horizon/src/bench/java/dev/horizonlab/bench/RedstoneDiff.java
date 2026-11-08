package dev.horizonlab.bench;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.phys.AABB;

/**
 * Dev-only: one machine of tools/redstone_diff.py at a time, in a frozen world. The client frame loop
 * (VanillaBench) freezes the tick, sends the machine's setup commands, waits a second for the server to
 * run them, and then takes turns with the server: one snapshot of the machine as the setup left it, then
 * for each tick {@code tick step 1} and a snapshot of the tick's result. A frozen server still calls
 * {@code ServerLevel.tick} every 50 ms and does nothing in it, so a tick counts only when the game time
 * moved during the call. A snapshot is the box's non-air blocks as "x y z id[property=value,...]", with
 * the properties sorted by name, and the entities in or beside the box with their position and motion;
 * tools/redstone_diff.py reads them from the events file.
 */
public final class RedstoneDiff {
    private static RedstoneDiff active;

    private final JsonObject machine;
    private final int[] box;
    private final int ticks;
    private final JsonArray snapshots = new JsonArray();
    private long setupAt;
    private boolean setup;
    /** The server may take the snapshot of the machine as set up. */
    private volatile boolean wantStart;
    private volatile boolean started;
    private boolean stepIssued;
    /** The game time when the current {@code ServerLevel.tick} call began. */
    private long headTime;
    private volatile int finished;

    RedstoneDiff(JsonObject machine) {
        this.machine = machine;
        JsonArray b = machine.getAsJsonArray("box");
        this.box = new int[6];
        for (int i = 0; i < 6; i++) box[i] = b.get(i).getAsInt();
        this.ticks = machine.get("ticks").getAsInt();
    }

    static void begin(RedstoneDiff diff) {
        active = diff;
    }

    static void end() {
        active = null;
    }

    /** Called at the head and the return of every {@code ServerLevel.tick}. */
    public static void level(ServerLevel level, boolean head) {
        RedstoneDiff d = active;
        if (d == null || level.dimension() != Level.OVERWORLD) return;
        if (head) {
            d.headTime = level.getGameTime();
            if (d.wantStart && !d.started) {
                d.snapshots.add(d.snapshot(level, 0));
                d.started = true;
            }
        } else if (d.started && level.getGameTime() != d.headTime) {
            d.snapshots.add(d.snapshot(level, d.finished + 1));
            d.stepIssued = false;
            d.finished++;
        }
    }

    /** One call per client frame: true once the machine's ticks are all stepped and snapshotted. */
    boolean drive(Minecraft mc) {
        if (!setup) {
            mc.player.connection.sendCommand("tick freeze");
            for (var line : machine.getAsJsonArray("setup")) mc.player.connection.sendCommand(line.getAsString());
            setup = true;
            setupAt = System.nanoTime();
            return false;
        }
        if (!wantStart) {
            wantStart = System.nanoTime() - setupAt > 1_000_000_000L;
            return false;
        }
        if (!started) return false;
        if (finished >= ticks) return true;
        if (!stepIssued) {
            stepIssued = true;
            // The commands scheduled for this tick, before the tick runs: the server takes them in order.
            if (machine.has("events") && machine.getAsJsonObject("events").has(String.valueOf(finished))) {
                for (var line : machine.getAsJsonObject("events").getAsJsonArray(String.valueOf(finished))) {
                    mc.player.connection.sendCommand(line.getAsString());
                }
            }
            mc.player.connection.sendCommand("tick step 1");
        }
        return false;
    }

    JsonObject result() {
        JsonObject e = new JsonObject();
        e.addProperty("machine", machine.get("name").getAsString());
        e.add("snapshots", snapshots);
        return e;
    }

    private JsonObject snapshot(ServerLevel level, int tick) {
        JsonObject s = new JsonObject();
        s.addProperty("tick", tick);
        s.addProperty("game_time", level.getGameTime());
        List<String> blocks = new ArrayList<>();
        BlockPos.MutableBlockPos at = new BlockPos.MutableBlockPos();
        for (int x = box[0]; x <= box[3]; x++) {
            for (int y = box[1]; y <= box[4]; y++) {
                for (int z = box[2]; z <= box[5]; z++) {
                    at.set(x, y, z);
                    BlockState state = level.getBlockState(at);
                    if (!state.isAir()) blocks.add(x + " " + y + " " + z + " " + describe(state));
                }
            }
        }
        JsonArray list = new JsonArray();
        blocks.forEach(list::add);
        s.add("blocks", list);
        JsonArray entities = new JsonArray();
        AABB area = new AABB(box[0], box[1], box[2], box[3] + 1, box[4] + 1, box[5] + 1).inflate(1.0);
        List<String> found = new ArrayList<>();
        for (Entity entity : level.getEntities((Entity) null, area, e -> true)) {
            var m = entity.getDeltaMovement();
            found.add(String.format(java.util.Locale.ROOT, "%s %.4f %.4f %.4f v %.4f %.4f %.4f ground=%b age=%d",
                net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()),
                entity.getX(), entity.getY(), entity.getZ(), m.x, m.y, m.z, entity.onGround(), entity.tickCount));
        }
        found.sort(null);
        found.forEach(entities::add);
        s.add("entities", entities);
        return s;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static String describe(BlockState state) {
        StringBuilder out = new StringBuilder(net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
        List<String> props = new ArrayList<>();
        for (Property property : state.getProperties()) {
            props.add(property.getName() + "=" + property.getName(state.getValue(property)));
        }
        if (!props.isEmpty()) {
            props.sort(null);
            out.append('[').append(String.join(",", props)).append(']');
        }
        return out.toString();
    }
}
