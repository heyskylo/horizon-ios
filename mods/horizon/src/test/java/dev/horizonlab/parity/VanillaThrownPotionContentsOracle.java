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
import net.minecraft.world.damagesource.*;
import net.minecraft.world.effect.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.projectile.arrow.*;
import net.minecraft.world.entity.projectile.throwableitemprojectile.*;
import net.minecraft.world.item.*;
import net.minecraft.world.item.alchemy.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.phys.*;

/** Isolated calls into checksum-pinned vanilla methods. No MinecraftServer object,
 * constructor, main, spin, world load, networking or EULA is used. Unsafe only
 * supplies geometry and recording endpoints that the real methods call. */
public final class VanillaThrownPotionContentsOracle {
    static Object unsafe;
    static Method allocate;
    static Field field(Class<?> c, String n) throws Exception {
        var f = c.getDeclaredField(n); f.setAccessible(true); return f;
    }
    static <T> T bare(Class<T> c) throws Exception { return c.cast(allocate.invoke(unsafe, c)); }
    static final class Capture {
        final List<MobEffectInstance> effects = new ArrayList<>();
        float delta;
        int calls;
    }
    static final class Sources extends DamageSources {
        DamageSource source;
        Sources() { super(null); }
        @Override public DamageSource magic() { return source; }
        @Override public DamageSource indirectMagic(Entity a, Entity b) { return source; }
    }
    static final class MobTarget extends LivingEntity {
        Capture capture; Sources sources;
        MobTarget() { super(null, null); }
        @Override public HumanoidArm getMainArm() { return HumanoidArm.RIGHT; }
        @Override public boolean isAffectedByPotions() { return true; }
        @Override public boolean isInvertedHealAndHarm() { return false; }
        @Override public DamageSources damageSources() { return sources; }
        @Override public boolean addEffect(MobEffectInstance e, Entity source) { capture.effects.add(e); return true; }
        @Override public void heal(float amount) { capture.delta += amount; capture.calls++; }
        @Override public boolean hurtServer(ServerLevel l, DamageSource s, float amount) { capture.delta -= amount; capture.calls++; return true; }
    }
    static final class PlayerTarget extends ServerPlayer {
        Capture capture; Sources sources;
        PlayerTarget() { super(null, null, null, null); }
        @Override public boolean isSpectator() { return false; }
        @Override public boolean isAffectedByPotions() { return true; }
        @Override public boolean isInvertedHealAndHarm() { return false; }
        @Override public DamageSources damageSources() { return sources; }
        @Override public boolean addEffect(MobEffectInstance e, Entity source) { capture.effects.add(e); return true; }
        @Override public void heal(float amount) { capture.delta += amount; capture.calls++; }
        @Override public boolean hurtServer(ServerLevel l, DamageSource s, float amount) { capture.delta -= amount; capture.calls++; return true; }
    }
    static final class Level extends ServerLevel {
        List<LivingEntity> targets;
        Entity added;
        Level() { super(null,null,null,null,null,null,false,0L,List.of(),false); }
        @Override public boolean isClientSide() { return false; }
        @Override public int getNextEntityId() { return 1; }
        @Override public net.minecraft.util.RandomSource getRandom() { return net.minecraft.util.RandomSource.create(1); }
        @Override public <T extends Entity> List<T> getEntities(EntityTypeTest<Entity,T> type, AABB box, Predicate<? super T> test) {
            var out = new ArrayList<T>();
            for (var target : targets) { T t = type.tryCast(target); if (t != null && box.intersects(t.getBoundingBox()) && test.test(t)) out.add(t); }
            return out;
        }
        @Override public BlockState getBlockState(BlockPos p) { return Blocks.STONE.defaultBlockState(); }
        @Override public boolean isRainingAt(BlockPos p) { return false; }
        @Override public boolean addFreshEntity(Entity e) { added = e; return true; }
        @Override public void broadcastEntityEvent(Entity e, byte event) { if (event != 0) throw new AssertionError(event); }
    }
    static final class GroundArrow extends Arrow {
        GroundArrow(Level l, ItemStack stack) { super(l, 0.5,64.5,0.5,stack,null); }
        // Environmental block effects are outside this grounded-expiry unit.
        @Override protected void applyEffectsFromBlocks() { }
        @Override public boolean isInWaterOrRain() { return false; }
    }
    static void geometry(Entity e, Level l, Vec3 at, double width, double height) throws Exception {
        field(Entity.class,"level").set(e,l);
        field(Entity.class,"position").set(e,at);
        field(Entity.class,"blockPosition").set(e,BlockPos.containing(at));
        field(Entity.class,"bb").set(e,new AABB(at.x-width/2,at.y,at.z-width/2,at.x+width/2,at.y+height,at.z+width/2));
    }
    static Holder<MobEffect> effect(String name) {
        return switch(name) { case "speed" -> MobEffects.SPEED; case "poison" -> MobEffects.POISON; case "glowing" -> MobEffects.GLOWING;
            case "instant_health" -> MobEffects.INSTANT_HEALTH; case "instant_damage" -> MobEffects.INSTANT_DAMAGE; default -> throw new AssertionError(name); };
    }
    static PotionContents contents(JsonObject row) {
        Optional<Holder<Potion>> potion = row.has("potion") ? Optional.of(switch(row.get("potion").getAsString()) {
            case "water" -> Potions.WATER; case "swiftness" -> Potions.SWIFTNESS; default -> throw new AssertionError(row); }) : Optional.empty();
        var custom = new ArrayList<MobEffectInstance>();
        for (var value : row.getAsJsonArray("custom")) { var e=value.getAsJsonObject(); custom.add(new MobEffectInstance(effect(e.get("id").getAsString()),e.get("duration").getAsInt(),e.get("amp").getAsInt(),e.get("ambient").getAsBoolean(),e.get("visible").getAsBoolean())); }
        return new PotionContents(potion,row.has("color")?Optional.of(row.get("color").getAsInt()):Optional.empty(),custom,row.has("name")?Optional.of(row.get("name").getAsString()):Optional.empty());
    }
    static JsonArray effects(Iterable<MobEffectInstance> effects) {
        var out=new JsonArray();
        for (var e:effects) { var v=new JsonObject(); v.addProperty("id",BuiltInRegistries.MOB_EFFECT.getKey(e.getEffect().value()).getPath()); v.addProperty("duration",e.getDuration()); v.addProperty("amp",e.getAmplifier()); v.addProperty("ambient",e.isAmbient()); v.addProperty("visible",e.isVisible()); out.add(v); }
        return out;
    }
    static JsonObject capture(Capture c) { var v=new JsonObject(); v.addProperty("delta",c.delta); v.add("effects",effects(c.effects)); return v; }
    public static void main(String[] args) throws Exception {
        ParityData.init(); // bind registry/component data, no MinecraftServer or world
        var u=Class.forName("sun.misc.Unsafe"); var f=u.getDeclaredField("theUnsafe"); f.setAccessible(true); unsafe=f.get(null); allocate=u.getMethod("allocateInstance",Class.class);
        var level=bare(Level.class); level.targets=new ArrayList<>();
        field(net.minecraft.world.level.Level.class,"registryAccess").set(level,ParityData.access);
        var sources=bare(Sources.class); sources.source=new DamageSource(Holder.direct(new DamageType("magic",0F)));
        var player=bare(PlayerTarget.class); player.sources=sources;
        var mob=bare(MobTarget.class); mob.sources=sources;
        geometry(player,level,new Vec3(.5,64,.5),(double).6F,(double)1.8F);
        geometry(mob,level,new Vec3(.5,64,.5),(double).6F,(double)1.95F);
        level.targets=List.of(player,mob);
        BuiltInRegistries.POTION.prepareTagReload(new net.minecraft.tags.TagLoader.LoadResult<>(Registries.POTION,Map.of(net.minecraft.tags.PotionTags.DOUSES_FIRE,List.<Holder<Potion>>of(Potions.WATER)))).apply();
        var output=new JsonArray();
        for (var input : JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray()) {
            var row=input.getAsJsonObject(); var pc=contents(row); var result=new JsonObject(); result.addProperty("id",row.get("id").getAsString());
            String kind=row.get("kind").getAsString();
            var stack=new ItemStack(kind.equals("lingering")?Items.LINGERING_POTION:kind.equals("arrow")?Items.TIPPED_ARROW:Items.SPLASH_POTION);
            stack.set(DataComponents.POTION_CONTENTS,pc); stack.set(DataComponents.POTION_DURATION_SCALE,row.get("scale").getAsFloat());
            if (kind.equals("arrow")) {
                var arrow=new GroundArrow(level,stack);
                field(AbstractArrow.class,"lastState").set(arrow,Blocks.STONE.defaultBlockState());
                field(AbstractArrow.class,"inGroundTime").setInt(arrow,598);
                arrow.tick(); result.addProperty("item599",BuiltInRegistries.ITEM.getKey(arrow.getPickupItemStackOrigin().getItem()).getPath()); result.addProperty("color599",arrow.getColor());
                arrow.tick(); result.addProperty("item600",BuiltInRegistries.ITEM.getKey(arrow.getPickupItemStackOrigin().getItem()).getPath()); result.addProperty("color600",arrow.getColor());
            } else if (kind.equals("lingering")) {
                var potion=bare(ThrownLingeringPotion.class); geometry(potion,level,new Vec3(.5,64.5,.5),.25,.25); level.added=null;
                potion.onHitAsPotion(level,stack,BlockHitResult.miss(potion.position(),Direction.UP,BlockPos.containing(potion.position())));
                var cloud=(AreaEffectCloud)level.added;
                result.addProperty("scale",cloud.get(DataComponents.POTION_DURATION_SCALE));
                var copied=cloud.get(DataComponents.POTION_CONTENTS); result.addProperty("color",copied.getColor()); result.add("effects",effects(copied.getAllEffects()));
            } else {
                player.capture=new Capture(); mob.capture=new Capture();
                double x=row.has("half")&&row.get("half").getAsBoolean()?2.925:.5;
                var potion=bare(ThrownSplashPotion.class); geometry(potion,level,new Vec3(x,64.5,.5),.25,.25); potion.tickCount=1;
                potion.onHitAsPotion(level,stack,BlockHitResult.miss(potion.position(),Direction.UP,BlockPos.containing(potion.position())));
                boolean instant=pc.customEffects().stream().anyMatch(e->e.getEffect().value().isInstantaneous());
                if (instant && (player.capture.calls!=1 || mob.capture.calls!=1)) throw new AssertionError("actual instantaneous method was not observed on both targets: "+row);
                result.add("player",capture(player.capture)); result.add("mob",capture(mob.capture)); result.addProperty("color",pc.getColor()); result.addProperty("empty",pc.equals(PotionContents.EMPTY)); result.addProperty("water",pc.is(net.minecraft.tags.PotionTags.DOUSES_FIRE));
            }
            output.add(result);
        }
        Files.writeString(Path.of(args[1]),new GsonBuilder().setPrettyPrinting().create().toJson(output));
    }
}
