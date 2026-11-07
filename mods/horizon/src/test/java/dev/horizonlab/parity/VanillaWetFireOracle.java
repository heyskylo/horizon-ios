package dev.horizonlab.parity;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.*;
import net.minecraft.world.level.material.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.phys.*;
import net.minecraft.world.damagesource.*;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.tags.TagKey;

public final class VanillaWetFireOracle {
    static MinecraftServer SERVER;
    static class Scene extends ServerLevel {
        boolean water, rain, lava, fire;
        int sounds;
        net.minecraft.world.level.chunk.ProtoChunk chunk;
        int next;
        Scene() { super(null,null,null,null,null,null,false,0L,List.of(),false); }
        @Override public int getNextEntityId() { return ++next; }
        @Override public RegistryAccess registryAccess() { return ParityData.access; }
        @Override public MinecraftServer getServer() { return SERVER; }
        @Override public boolean isClientSide() { return false; }
        @Override public BlockState getBlockState(BlockPos p) { return water ? Blocks.WATER.defaultBlockState() : lava ? Blocks.LAVA.defaultBlockState() : fire ? Blocks.FIRE.defaultBlockState() : Blocks.AIR.defaultBlockState(); }
        @Override public FluidState getFluidState(BlockPos p) { return getBlockState(p).getFluidState(); }
        @Override public net.minecraft.world.level.chunk.ChunkAccess getChunk(int x,int z,net.minecraft.world.level.chunk.status.ChunkStatus status,boolean create) { return chunk; }
        @Override public boolean hasChunkAt(BlockPos p) { return true; }
        @Override public boolean hasChunk(int x, int z) { return true; }
        @Override public boolean isRainingAt(BlockPos p) { return rain; }
        @Override public DamageSources damageSources() { return new DamageSources(ParityData.access); }
        @Override public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() {
            return net.minecraft.world.attribute.EnvironmentAttributeSystem.builder()
                .addConstantLayer(net.minecraft.world.attribute.EnvironmentAttributes.FAST_LAVA, previous -> false).build();
        }
        @Override public int getMinY() { return -64; }
        @Override public net.minecraft.world.Difficulty getDifficulty() { return net.minecraft.world.Difficulty.NORMAL; }
        @Override public java.util.List<Entity> getEntities(Entity except, AABB bounds, java.util.function.Predicate<? super Entity> predicate) { return List.of(); }
        @Override public long getGameTime() { return 0; }
        @Override public void gameEvent(Holder<net.minecraft.world.level.gameevent.GameEvent> event, Vec3 pos, net.minecraft.world.level.gameevent.GameEvent.Context context) {}
        @Override public void broadcastEntityEvent(Entity entity, byte event) {}
        @Override public void broadcastDamageEvent(Entity entity, DamageSource source) {}
        @Override public void playSound(Entity except, double x, double y, double z, SoundEvent sound, net.minecraft.sounds.SoundSource source, float volume, float pitch) { if(sound == net.minecraft.sounds.SoundEvents.GENERIC_EXTINGUISH_FIRE) sounds++; }
        @Override public void playSound(Entity except, double x, double y, double z, Holder<SoundEvent> sound, net.minecraft.sounds.SoundSource source, float volume, float pitch) { if(sound.value() == net.minecraft.sounds.SoundEvents.GENERIC_EXTINGUISH_FIRE) sounds++; }
    }
    static net.minecraft.world.entity.ai.navigation.PathNavigation NAV;
    static class Nav extends net.minecraft.world.entity.ai.navigation.GroundPathNavigation {
        Nav() { super(null,null); }
        @Override public void tick() {}
        @Override public void stop() {}
    }
    static class Actor extends Mob {
        Scene scene;
        boolean immune;
        int damage, sounds;
        DamageSources sources;
        Actor(Scene scene) { super(EntityTypes.PIG,scene); this.scene=scene; sources=new DamageSources(ParityData.access); }
        @Override protected void registerGoals() {}
        @Override protected net.minecraft.world.entity.ai.navigation.PathNavigation createNavigation(Level level) { return NAV; }
        @Override public boolean fireImmune() { return immune; }
        @Override protected void handlePortal() {}
        @Override public void checkBelowWorld() {}
        @Override public void travel(Vec3 input) {}
        @Override protected void pushEntities() {}
        @Override public boolean isInLava() { return scene.lava; }
        @Override public DamageSources damageSources() { return sources; }
        @Override public boolean hurtServer(ServerLevel level, DamageSource source, float amount) { damage += (int)amount; return super.hurtServer(level,source,amount); }
        @Override public void playSound(SoundEvent sound, float volume, float pitch) { sounds++; }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var u=Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");u.setAccessible(true);var unsafe=u.get(null);
        var scene=(Scene)unsafe.getClass().getMethod("allocateInstance",Class.class).invoke(unsafe,Scene.class);
        NAV=(Nav)unsafe.getClass().getMethod("allocateInstance",Class.class).invoke(unsafe,Nav.class);
        SERVER=(MinecraftServer)unsafe.getClass().getMethod("allocateInstance",Class.class).invoke(unsafe,net.minecraft.server.dedicated.DedicatedServer.class);
        var debug=MinecraftServer.class.getDeclaredField("debugSubscribers");debug.setAccessible(true);debug.set(SERVER,new net.minecraft.util.debug.ServerDebugSubscribers(SERVER));
        var biome=net.minecraft.core.Holder.Reference.createStandAlone(new net.minecraft.core.HolderOwner<net.minecraft.world.level.biome.Biome>(){},net.minecraft.world.level.biome.Biomes.PLAINS);
        var ids=new net.minecraft.core.IdMapper<net.minecraft.core.Holder<net.minecraft.world.level.biome.Biome>>();ids.add(biome);
        var factory=new net.minecraft.world.level.chunk.PalettedContainerFactory(net.minecraft.world.level.chunk.Strategy.createForBlockStates(Block.BLOCK_STATE_REGISTRY),Blocks.AIR.defaultBlockState(),null,net.minecraft.world.level.chunk.Strategy.createForBiomes(ids),biome,null);
        scene.chunk=new net.minecraft.world.level.chunk.ProtoChunk(new ChunkPos(0,0),net.minecraft.world.level.chunk.UpgradeData.EMPTY,LevelHeightAccessor.create(-64,384),factory,null);
        for(var section:scene.chunk.getSections()) section.setBlockState(15,15,15,Blocks.WATER.defaultBlockState(),false);
        for(boolean noAi:new boolean[]{true,false}) for(String wet:new String[]{"dry","water","rain"}) for(int fire:new int[]{0,1,19,20,21,40}) {
            scene.water=wet.equals("water");scene.rain=wet.equals("rain");scene.sounds=0;
            var actor=new Actor(scene);actor.setPos(0.5,64,0.5);actor.setNoAi(noAi);actor.setRemainingFireTicks(fire);actor.getRandom().setSeed(1);
            actor.tick();
            System.out.println("RESULT "+noAi+" "+wet+" "+fire+" health="+actor.getHealth()+" damage="+actor.damage+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds+" next="+actor.getRandom().nextLong());
        }
        for(boolean noAi:new boolean[]{true,false}) for(String wet:new String[]{"water","rain"}) {
            scene.water=false;scene.rain=false;scene.sounds=0;
            var actor=new Actor(scene);actor.setPos(0.5,64,0.5);actor.setNoAi(noAi);actor.setRemainingFireTicks(21);actor.getRandom().setSeed(1);
            actor.tick();
            System.out.println("TRANSITION "+noAi+" "+wet+" dry health="+actor.getHealth()+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds);
            scene.water=wet.equals("water");scene.rain=wet.equals("rain");scene.sounds=0;
            actor.tick();
            System.out.println("TRANSITION "+noAi+" "+wet+" wet health="+actor.getHealth()+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds);
            scene.water=false;scene.rain=false;scene.sounds=0;
            actor.tick();
            System.out.println("TRANSITION "+noAi+" "+wet+" dry_again health="+actor.getHealth()+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds);
        }
        for(boolean noAi:new boolean[]{true,false}) for(String wet:new String[]{"dry","water","rain"}) {
            scene.water=wet.equals("water");scene.rain=wet.equals("rain");scene.sounds=0;
            var actor=new Actor(scene);actor.immune=true;actor.setPos(0.5,64,0.5);actor.setNoAi(noAi);actor.setRemainingFireTicks(20);actor.getRandom().setSeed(1);
            actor.tick();
            System.out.println("IMMUNE "+noAi+" "+wet+" health="+actor.getHealth()+" damage="+actor.damage+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds);
        }
        for(boolean noAi:new boolean[]{true,false}) {
            for(String environment:new String[]{"lava_dry","lava_rain","fire_dry","fire_rain"}) for(int fire:new int[]{0,1,20}) {
                scene.water=false;scene.rain=environment.endsWith("_rain");scene.lava=environment.startsWith("lava_");scene.fire=environment.startsWith("fire_");scene.sounds=0;
                var actor=new Actor(scene);actor.setPos(0.5,64.2,0.5);actor.setNoAi(noAi);actor.setRemainingFireTicks(fire);actor.getRandom().setSeed(1);
                actor.tick();
                System.out.println("RESULT "+noAi+" "+environment+" "+fire+" health="+actor.getHealth()+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds);
            }
            scene.lava=true;scene.fire=false;scene.rain=true;scene.sounds=0;
            var actor=new Actor(scene);actor.setPos(0.5,64.2,0.5);actor.setNoAi(noAi);actor.setRemainingFireTicks(0);actor.getRandom().setSeed(1);
            for(int tick=0;tick<2;tick++) {
                scene.sounds=0;actor.tick();
                System.out.println("RESULT "+noAi+" lava_rain_repeat 0 health="+actor.getHealth()+" fire="+actor.getRemainingFireTicks()+" sounds="+scene.sounds);
            }
            scene.lava=false;scene.fire=false;
        }
    }
}
