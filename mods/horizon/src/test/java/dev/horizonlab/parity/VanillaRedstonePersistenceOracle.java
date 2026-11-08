package dev.horizonlab.parity;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.function.Predicate;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BarrelBlockEntity;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.ComparatorBlockEntity;
import net.minecraft.world.level.block.entity.HopperBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.storage.TagValueInput;
import net.minecraft.world.phys.AABB;

/** Native load/save and pushItemsTick, without a running server or player data. */
public final class VanillaRedstonePersistenceOracle {
    static final BlockPos HOPPER = new BlockPos(3, 64, 3);
    static final BlockPos BARREL = HOPPER.below();
    static final class World extends net.minecraft.server.level.ServerLevel {
        long time;
        BarrelBlockEntity barrel;
        World() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public boolean isClientSide() { return false; }
        @Override public long getGameTime() { return time; }
        @Override public BlockState getBlockState(BlockPos p) {
            return p.equals(BARREL) ? Blocks.BARREL.defaultBlockState() : Blocks.AIR.defaultBlockState();
        }
        @Override public BlockEntity getBlockEntity(BlockPos p) { return p.equals(BARREL) ? barrel : null; }
        @Override public List<Entity> getEntities(Entity except, AABB box, Predicate<? super Entity> predicate) { return List.of(); }
        @Override public <T extends Entity> List<T> getEntities(net.minecraft.world.level.entity.EntityTypeTest<Entity, T> type,
                AABB box, Predicate<? super T> predicate) { return List.of(); }
        @Override public void blockEntityChanged(BlockPos p) { }
        @Override public void updateNeighbourForOutputSignal(BlockPos p, Block b) { }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var registries = ParityData.lookup;
        var pos = new BlockPos(1, 64, 1);
        var lines = new ArrayList<String>();
        net.minecraft.nbt.Tag[] values = { null, net.minecraft.nbt.ByteTag.valueOf((byte)-1),
            net.minecraft.nbt.ShortTag.valueOf((short)23), net.minecraft.nbt.IntTag.valueOf(Integer.MIN_VALUE),
            net.minecraft.nbt.LongTag.valueOf(4294967297L), net.minecraft.nbt.FloatTag.valueOf(-1.5f),
            net.minecraft.nbt.DoubleTag.valueOf(15.75), net.minecraft.nbt.StringTag.valueOf("wrong") };
        for (int index = 0; index < values.length; index++) {
            var input = new CompoundTag();
            if (values[index] != null) { input.put("OutputSignal", values[index]); input.put("TransferCooldown", values[index]); }
            var c = new ComparatorBlockEntity(pos, Blocks.COMPARATOR.defaultBlockState());
            var h = new HopperBlockEntity(HOPPER, Blocks.HOPPER.defaultBlockState());
            c.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING, registries, input));
            h.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING, registries, input));
            lines.add("integer " + index + " " + c.saveWithFullMetadata(registries).getIntOr("OutputSignal", 0)
                + " " + h.saveWithFullMetadata(registries).getIntOr("TransferCooldown", -1));
        }
        var comparator = new ComparatorBlockEntity(pos, Blocks.COMPARATOR.defaultBlockState());
        comparator.setOutputSignal(15);
        CompoundTag comparatorTag = comparator.saveWithFullMetadata(registries);
        comparator = new ComparatorBlockEntity(pos, Blocks.COMPARATOR.defaultBlockState());
        comparator.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING, registries, comparatorTag));
        var hopper = new HopperBlockEntity(HOPPER, Blocks.HOPPER.defaultBlockState());
        for (int slot = 0; slot < 5; slot++) hopper.setItem(slot, new ItemStack(Items.STONE, 64));
        CompoundTag saved = hopper.saveWithFullMetadata(registries);
        saved.putInt("TransferCooldown", 4);
        hopper = new HopperBlockEntity(HOPPER, Blocks.HOPPER.defaultBlockState());
        hopper.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING, registries, saved));
        saved = hopper.saveWithFullMetadata(registries);
        var field = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        field.setAccessible(true);
        Object unsafe = field.get(null);
        World world = (World) unsafe.getClass().getMethod("allocateInstance", Class.class).invoke(unsafe, World.class);
        world.barrel = new BarrelBlockEntity(BARREL, Blocks.BARREL.defaultBlockState());
        hopper.setLevel(world);
        lines.add("state " + comparator.getOutputSignal() + " " + saved.getIntOr("TransferCooldown", -1) + " 0");
        for (int tick = 1; tick <= 6; tick++) {
            world.time++;
            HopperBlockEntity.pushItemsTick(world, HOPPER, Blocks.HOPPER.defaultBlockState(), hopper);
            int count = 0;
            for (int slot = 0; slot < 27; slot++) count += world.barrel.getItem(slot).getCount();
            lines.add("tick " + tick + " " + hopper.saveWithFullMetadata(registries).getIntOr("TransferCooldown", -1) + " " + count);
        }
        Files.writeString(Path.of(args[0]), String.join("\n", lines) + "\n");
    }
}
