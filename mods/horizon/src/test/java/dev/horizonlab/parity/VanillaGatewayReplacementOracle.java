package dev.horizonlab.parity;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.TheEndGatewayBlockEntity;

/** Gateway instance lifecycle only: no level, MinecraftServer, ticks or networking. */
public final class VanillaGatewayReplacementOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var state = Blocks.END_GATEWAY.defaultBlockState();
        var first = new TheEndGatewayBlockEntity(new BlockPos(2, 200, 2), state);
        var untouched = new TheEndGatewayBlockEntity(new BlockPos(4, 200, 2), state);
        boolean fresh = first.isCoolingDown();
        first.triggerEvent(1, 0);
        untouched.triggerEvent(1, 0);
        boolean entered = first.isCoolingDown();
        first.setRemoved();
        var replacement = new TheEndGatewayBlockEntity(new BlockPos(2, 200, 2), state);
        Files.write(Path.of(args[0]), List.of(
            "fresh " + fresh,
            "entered " + entered,
            "removed " + first.isRemoved(),
            "replacement " + replacement.isCoolingDown(),
            "untouched " + untouched.isCoolingDown()
        ));
    }
}
