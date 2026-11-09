package dev.horizonlab.parity;

import java.util.List;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;

/** Checks the shipped onPlace methods on a powered client level, without a game window. */
public final class VanillaClientRedstonePreviewOracle {
    static final class World extends ClientLevel {
        World() { super(null, null, null, null, 0, 0, null, false, 0, 0); }
        @Override public boolean isClientSide() { return true; }
        @Override public boolean hasNeighborSignal(BlockPos pos) { return true; }
        @Override public BlockState getBlockState(BlockPos pos) { return Blocks.AIR.defaultBlockState(); }
        @Override public boolean removeBlock(BlockPos pos, boolean moved) {
            throw new AssertionError("client placement must not remove powered TNT");
        }
        @Override public void blockEvent(BlockPos pos, Block block, int event, int direction) {
            throw new AssertionError("client placement must not start a piston move");
        }
    }

    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var field = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        field.setAccessible(true);
        var unsafe = field.get(null);
        var allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
        for (var block : List.of(Blocks.TNT, Blocks.PISTON, Blocks.STICKY_PISTON)) {
            var world = (World) allocate.invoke(unsafe, World.class);
            block.defaultBlockState().onPlace(world, BlockPos.ZERO, Blocks.AIR.defaultBlockState(), false);
        }
    }
}
