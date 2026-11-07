package dev.horizonlab.parity;
import com.google.gson.JsonParser;
import java.nio.file.*;
import java.io.PrintWriter;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.tags.*;
import net.minecraft.world.effect.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.material.Fluid;
public final class VanillaMobBreathingOracle {
    static final class Finished extends RuntimeException {}
    static final class Actor extends Cow {
        int mask, scene;
        boolean reached;
        Actor(Level level) { super(EntityTypes.COW, level); }
        @Override protected boolean updateFluidInteraction() { return false; }
        @Override public boolean isEyeInFluid(TagKey<Fluid> fluid) { return scene != 0 && fluid == FluidTags.WATER; }
        @Override public boolean hasEffect(Holder<MobEffect> effect) {
            return ((mask & 1) != 0 && effect == MobEffects.WATER_BREATHING)
                || ((mask & 2) != 0 && effect == MobEffects.CONDUIT_POWER)
                || ((mask & 4) != 0 && effect == MobEffects.BREATH_OF_THE_NAUTILUS);
        }
        @Override protected void tickEffects() { reached = true; throw new Finished(); }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        VanillaBlockUpdateOracle.OVERWORLD_TYPE = VanillaBlockUpdateOracle.loadOverworld();
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe"); field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        try(var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            for(int scene=0;scene<5;scene++) for(int mask=0;mask<8;mask++) for(int air:new int[]{301,300,10,0,-19,-20,-21}) {
                var world = VanillaBlockUpdateOracle.build(JsonParser.parseString("{\"cells\":[],\"seed\":0}").getAsJsonObject());
                world.entities = new ArrayList<>(); world.registries = ParityData.access;
                VanillaBlockUpdateOracle.set(Level.class,world,"damageSources",new net.minecraft.world.damagesource.DamageSources(ParityData.access));
                world.cells.put(new BlockPos(2,65,2),(scene==2||scene==3?Blocks.BUBBLE_COLUMN:scene!=0?Blocks.WATER:Blocks.AIR).defaultBlockState());
                world.cells.put(new BlockPos(2,64,2),(scene==2||scene==4?Blocks.BUBBLE_COLUMN:scene!=0?Blocks.WATER:Blocks.AIR).defaultBlockState());
                var actor = new Actor(world); actor.mask=mask;actor.scene=scene;
                actor.setPos(2.5,64,2.5); actor.setNoAi(true);actor.setAirSupply(air);
                try { actor.baseTick(); } catch(Finished done) {}
                if(!actor.reached) throw new AssertionError("breathing phase not reached");
                out.printf(Locale.ROOT,"%d %d %d %d %08x%n",scene,mask,air,actor.getAirSupply(),Float.floatToIntBits(actor.getHealth()));
            }
        }
    }
}
