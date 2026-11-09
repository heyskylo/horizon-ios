package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import net.minecraft.core.Holder;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;

/** Real Cow.aiStep through travel and movement, stopped before block contacts.
 * The scene is dry air and gravity is disabled to isolate accumulated fall distance.
 * Active-effect lookups are controlled; the NoAI flag and movement are vanilla's.
 */
public final class VanillaMobFallResetOracle {
    static final class Finished extends RuntimeException {}

    static final class Actor extends Cow {
        String effect;
        boolean reachedTravelEnd;

        Actor(Level level) { super(EntityTypes.COW, level); }

        @Override public boolean hasEffect(Holder<MobEffect> query) {
            return "levitation".equals(effect) && query == MobEffects.LEVITATION
                || "slow_falling".equals(effect) && query == MobEffects.SLOW_FALLING;
        }

        @Override public MobEffectInstance getEffect(Holder<MobEffect> query) {
            return hasEffect(query) ? new MobEffectInstance(query, 100, 0) : null;
        }

        // The fixture contains only air; no fluid interaction is possible.
        @Override protected boolean updateFluidInteraction() { return false; }

        @Override protected void applyEffectsFromBlocks() {
            reachedTravelEnd = true;
            throw new Finished();
        }
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) throw new IllegalArgumentException("expected output path");
        ParityData.init();
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        VanillaBlockUpdateOracle.OVERWORLD_TYPE = VanillaBlockUpdateOracle.loadOverworld();
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            for (String effect : List.of("none", "levitation", "slow_falling")) {
                for (boolean noAI : List.of(false, true)) {
                    for (double vy : new double[]{0.0, 0.2, -0.2}) {
                        var world = VanillaBlockUpdateOracle.build(
                            JsonParser.parseString("{\"cells\":[],\"seed\":0}").getAsJsonObject());
                        world.entities = new ArrayList<>();
                        world.registries = ParityData.access;
                        var actor = new Actor(world);
                        actor.effect = effect;
                        actor.setNoAi(noAI);
                        actor.fallDistance = 12.0;
                        actor.setNoGravity(true);
                        actor.setPos(2.5, 64.0, 2.5);
                        actor.setDeltaMovement(0, vy, 0);
                        try { actor.aiStep(); } catch (Finished done) { }
                        if (!actor.reachedTravelEnd) {
                            throw new AssertionError("did not reach the post-travel boundary");
                        }
                        out.printf(Locale.ROOT, "%s %s %016x %016x%n", effect, noAI,
                            Double.doubleToLongBits(vy), Double.doubleToLongBits(actor.fallDistance));
                    }
                }
            }
        }
    }
}
