package dev.horizonlab.parity;

import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.item.PrimedTnt;
import net.minecraft.world.level.ExplosionDamageCalculator;
import net.minecraft.world.level.block.Blocks;

/** Query the actual 26.3 calculator without starting a world or server. */
public final class VanillaSulfurPortalBlastOracle {
    public static void main(String[] args) {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var normal = new ExplosionDamageCalculator();
        var portal = PrimedTnt.USED_PORTAL_DAMAGE_CALCULATOR;
        for (var block : new net.minecraft.world.level.block.Block[]{Blocks.NETHER_PORTAL, Blocks.STONE, Blocks.AIR}) {
            var state = block.defaultBlockState();
            for (var calculator : new ExplosionDamageCalculator[]{normal, portal}) {
                var resistance = calculator.getBlockExplosionResistance(null, null, BlockPos.ZERO, state, state.getFluidState());
                var breaks = calculator.shouldBlockExplode(null, null, BlockPos.ZERO, state, 4.0F);
                System.out.println(block + " " + (calculator == portal) + " " + resistance + " " + breaks);
                if (calculator == portal && block == Blocks.NETHER_PORTAL && (resistance.isPresent() || breaks)) {
                    throw new AssertionError("portal protection");
                }
                if (block != Blocks.NETHER_PORTAL && calculator == portal &&
                    (!resistance.equals(normal.getBlockExplosionResistance(null, null, BlockPos.ZERO, state, state.getFluidState())) || !breaks)) {
                    throw new AssertionError("other block changed");
                }
            }
        }
    }
}
