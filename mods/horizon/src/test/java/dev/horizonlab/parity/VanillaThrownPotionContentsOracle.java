package dev.horizonlab.parity;

import com.google.gson.*;
import java.nio.file.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.core.*;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.*;
import net.minecraft.server.level.*;
import net.minecraft.world.effect.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.projectile.arrow.*;
import net.minecraft.world.item.*;
import net.minecraft.world.item.alchemy.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.phys.*;

/** Actual Arrow.tick at ticks 599/600. No server construction or world bootstrap. */
public final class VanillaThrownPotionContentsOracle {
    static Object unsafe;
    static Method allocate;
    static Field field(Class<?> c, String n) throws Exception {
        var f = c.getDeclaredField(n); f.setAccessible(true); return f;
    }
    static <T> T bare(Class<T> c) throws Exception { return c.cast(allocate.invoke(unsafe, c)); }
    static final class Level extends ServerLevel {
        Level() { super(null,null,null,null,null,null,false,0L,List.of(),false); }
        @Override public boolean isClientSide() { return false; }
        @Override public int getNextEntityId() { return 1; }
        @Override public net.minecraft.util.RandomSource getRandom() { return net.minecraft.util.RandomSource.create(1); }
        @Override public <T extends Entity> List<T> getEntities(EntityTypeTest<Entity,T> type, AABB box, Predicate<? super T> test) { return List.of(); }
        @Override public BlockState getBlockState(BlockPos p) { return Blocks.STONE.defaultBlockState(); }
        @Override public boolean isRainingAt(BlockPos p) { return false; }
        @Override public void broadcastEntityEvent(Entity e, byte event) { if (event != 0) throw new AssertionError(event); }
    }
    static final class GroundArrow extends Arrow {
        GroundArrow(Level l, ItemStack stack) { super(l, 0.5,64.5,0.5,stack,null); }
        // Environmental block effects are outside this grounded-expiry unit.
        @Override protected void applyEffectsFromBlocks() { }
        @Override public boolean isInWaterOrRain() { return false; }
    }
    static PotionContents contents(JsonObject row) {
        var potion = row.has("potion") ? Optional.of(Potions.SWIFTNESS) : Optional.<Holder<Potion>>empty();
        var custom = new ArrayList<MobEffectInstance>();
        for (var value : row.getAsJsonArray("custom")) {
            var e = value.getAsJsonObject();
            if (!e.get("id").getAsString().equals("glowing")) throw new AssertionError(e);
            custom.add(new MobEffectInstance(MobEffects.GLOWING,e.get("duration").getAsInt(),e.get("amp").getAsInt(),e.get("ambient").getAsBoolean(),e.get("visible").getAsBoolean()));
        }
        return new PotionContents(potion,row.has("color")?Optional.of(row.get("color").getAsInt()):Optional.empty(),custom,row.has("name")?Optional.of(row.get("name").getAsString()):Optional.empty());
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var u=Class.forName("sun.misc.Unsafe"); var f=u.getDeclaredField("theUnsafe"); f.setAccessible(true); unsafe=f.get(null); allocate=u.getMethod("allocateInstance",Class.class);
        var level=bare(Level.class);
        field(net.minecraft.world.level.Level.class,"registryAccess").set(level,ParityData.access);
        var output=new JsonArray();
        for (var input : JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray()) {
            var row=input.getAsJsonObject();
            var stack=new ItemStack(Items.TIPPED_ARROW); stack.set(DataComponents.POTION_CONTENTS,contents(row));
            var arrow=new GroundArrow(level,stack);
            field(AbstractArrow.class,"lastState").set(arrow,Blocks.STONE.defaultBlockState());
            field(AbstractArrow.class,"inGroundTime").setInt(arrow,598);
            var result=new JsonObject(); result.addProperty("id",row.get("id").getAsString());
            arrow.tick(); result.addProperty("item599",BuiltInRegistries.ITEM.getKey(arrow.getPickupItemStackOrigin().getItem()).getPath()); result.addProperty("color599",arrow.getColor());
            arrow.tick(); result.addProperty("item600",BuiltInRegistries.ITEM.getKey(arrow.getPickupItemStackOrigin().getItem()).getPath()); result.addProperty("color600",arrow.getColor());
            output.add(result);
        }
        Files.writeString(Path.of(args[1]),new GsonBuilder().setPrettyPrinting().create().toJson(output));
    }
}
