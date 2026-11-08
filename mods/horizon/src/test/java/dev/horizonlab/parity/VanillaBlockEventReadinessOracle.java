package dev.horizonlab.parity;

import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import it.unimi.dsi.fastutil.objects.ObjectLinkedOpenHashSet;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.BlockEventData;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SculkSpreader;
import net.minecraft.world.level.block.state.BlockState;

/** Actual block-event dispatch and sculk cursor gates; no server or level constructor runs. */
public final class VanillaBlockEventReadinessOracle {
    static final class World extends ServerLevel {
        boolean ticketed;
        boolean ticking;
        int dispatched;
        World() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public boolean shouldTickBlocksAt(long chunk) { return ticketed; }
        @Override public boolean isPositionTickingWithEntitiesLoaded(long chunk) { return ticking; }
        @Override public BlockState getBlockState(BlockPos pos) {
            dispatched++;
            // A stale event deliberately avoids broadcasting or executing a block's effects.
            return Blocks.AIR.defaultBlockState();
        }
    }

    static Field field(Class<?> owner, String name) throws Exception {
        Field field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        Class<?> allocator = Class.forName("sun.misc.Unsafe");
        Object unsafe = field(allocator, "theUnsafe").get(null);
        var allocate = allocator.getMethod("allocateInstance", Class.class);
        var run = ServerLevel.class.getDeclaredMethod("runBlockEvents");
        run.setAccessible(true);
        var delay = field(SculkSpreader.ChargeCursor.class, "updateDelay");
        var out = new ArrayList<String>();
        for (boolean ticketed : new boolean[] {true, false}) {
            for (boolean ticking : new boolean[] {false, true}) {
                World world = (World) allocate.invoke(unsafe, World.class);
                world.ticketed = ticketed;
                world.ticking = ticking;
                var events = new ObjectLinkedOpenHashSet<BlockEventData>();
                events.add(new BlockEventData(BlockPos.ZERO, Blocks.PISTON, 0, 1));
                field(ServerLevel.class, "blockEvents").set(world, events);
                field(ServerLevel.class, "blockEventsToReschedule").set(world, new ArrayList<BlockEventData>());
                run.invoke(world);
                var cursor = new SculkSpreader.ChargeCursor(BlockPos.ZERO, 5);
                delay.setInt(cursor, 2);
                cursor.update(world, BlockPos.ZERO, RandomSource.create(42), SculkSpreader.createLevelSpreader(), false);
                out.add(ticketed + " " + ticking + " " + world.dispatched + " " + events.size() + " " + delay.getInt(cursor));
            }
        }
        Files.write(Path.of(args[0]), out);
    }
}
