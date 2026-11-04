package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.*;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.util.RandomSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.projectile.arrow.Arrow;
import net.minecraft.world.entity.projectile.arrow.AbstractArrow;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.dimension.*;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.*;
import net.minecraft.world.phys.shapes.VoxelShape;
import static dev.horizonlab.parity.VanillaRegistryCollisionScanOracle.*;

/** Original oracle: actual client-side Arrow.tick in a recording resident world. */
public final class VanillaClientArrowOracle {
    static final class World extends ClientLevel {
        Map<Long,Chunk> chunks;
        RandomSource rng;
        World() { super(null,null,null,null,0,0,null,false,0L,0); }
        @Override public boolean isClientSide() { return true; }
        @Override public int getNextEntityId() { return 1; }
        @Override public int getMinY() { return -64; }
        @Override public int getHeight() { return 384; }
        @Override public RegistryAccess registryAccess() { return ParityData.access; }
        @Override public DimensionType dimensionType() { return ParityData.access.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD).value(); }
        @Override public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() { return VanillaArrowOracle.Level.ATTRIBUTES; }
        @Override public RandomSource getRandom() { return rng; }
        @Override public BlockGetter getChunkForCollisions(int x,int z) { return chunks.get(key(x,z)); }
        @Override public ChunkAccess getChunk(int x,int z,net.minecraft.world.level.chunk.status.ChunkStatus status,boolean load) { return chunks.get(key(x,z)); }
        @Override public BlockState getBlockState(BlockPos p) { var c=chunks.get(key(p.getX()>>4,p.getZ()>>4)); return c==null?Blocks.AIR.defaultBlockState():c.getBlockState(p); }
        @Override public FluidState getFluidState(BlockPos p) { return getBlockState(p).getFluidState(); }
        @Override public WorldBorder getWorldBorder() { return new VanillaArrowOracle.Border(); }
        @Override public List<VoxelShape> getEntityCollisions(Entity entity,AABB bounds) { return List.of(); }
        @Override public List<Entity> getEntities(Entity entity,AABB bounds,java.util.function.Predicate<? super Entity> filter) { return List.of(); }
        @Override public boolean isRainingAt(BlockPos pos) { return false; }
        @Override public void addParticle(net.minecraft.core.particles.ParticleOptions particle,double x,double y,double z,double dx,double dy,double dz) { }
        @Override public void playSound(Entity except,double x,double y,double z,net.minecraft.sounds.SoundEvent event,net.minecraft.sounds.SoundSource source,float volume,float pitch) { }
        @Override public void playSound(Entity except,Entity entity,net.minecraft.sounds.SoundEvent event,net.minecraft.sounds.SoundSource source,float volume,float pitch) { }
        @Override public void gameEvent(Holder<net.minecraft.world.level.gameevent.GameEvent> event,Vec3 pos,net.minecraft.world.level.gameevent.GameEvent.Context context) { }
        @Override public void gameEvent(Holder<net.minecraft.world.level.gameevent.GameEvent> event,BlockPos pos,net.minecraft.world.level.gameevent.GameEvent.Context context) { }
    }
    static java.lang.reflect.Field field(Class<?> owner,String name) throws Exception { var f=owner.getDeclaredField(name);f.setAccessible(true);return f; }
    static String bits(Vec3 v) { return String.format("%016x,%016x,%016x",Double.doubleToRawLongBits(v.x),Double.doubleToRawLongBits(v.y),Double.doubleToRawLongBits(v.z)); }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var allocator=Class.forName("sun.misc.Unsafe");var unsafe=field(allocator,"theUnsafe").get(null);var allocate=allocator.getMethod("allocateInstance",Class.class);
        var biome=ParityData.access.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
        var ids=new IdMapper<Holder<net.minecraft.world.level.biome.Biome>>();ids.add(biome);
        factory=new PalettedContainerFactory(Strategy.createForBlockStates(Block.BLOCK_STATE_REGISTRY),Blocks.AIR.defaultBlockState(),null,Strategy.createForBiomes(ids),biome,null);
        var trace=new Trace();trace.out=new PrintWriter(java.io.OutputStream.nullOutputStream());trace.fail=-1;
        try(var out=new PrintWriter(Files.newBufferedWriter(Path.of(args[1]),StandardCharsets.UTF_8))) {
            for(var line:Files.readAllLines(Path.of(args[0]))) {
                var row=JsonParser.parseString(line).getAsJsonObject();
                var world=(World)allocate.invoke(unsafe,World.class);world.rng=RandomSource.create(42);world.chunks=new HashMap<>();
                field(Level.class,"random").set(world,world.rng);
                for(int x=-1;x<=1;x++)for(int z=-1;z<=1;z++)world.chunks.put(key(x,z),new Chunk(x,z,trace,null));
                for(var entry:row.getAsJsonArray("cells")) {
                    var c=entry.getAsJsonArray();int x=c.get(0).getAsInt(),y=c.get(1).getAsInt(),z=c.get(2).getAsInt();
                    var state=net.minecraft.core.registries.BuiltInRegistries.BLOCK.getValue(net.minecraft.resources.Identifier.parse(c.get(3).getAsString())).defaultBlockState();
                    if(c.size()>4) {
                        if(state.is(Blocks.BUBBLE_COLUMN))state=state.setValue(net.minecraft.world.level.block.state.properties.BlockStateProperties.DRAG,c.get(4).getAsBoolean());
                        else state=VanillaSceneCollisionOracle.property(state,net.minecraft.world.level.block.state.properties.BlockStateProperties.LEVEL,c.get(4).getAsString());
                    }
                    world.chunks.get(key(x>>4,z>>4)).write(new BlockPos(x,y,z),Block.getId(state));
                }
                var pos=row.getAsJsonArray("position");var v=row.getAsJsonArray("velocity");
                var arrow=new Arrow(world,pos.get(0).getAsDouble(),pos.get(1).getAsDouble(),pos.get(2).getAsDouble(),new ItemStack(Items.ARROW),null);
                field(Entity.class,"random").set(arrow,RandomSource.create(42));
                arrow.setDeltaMovement(new Vec3(v.get(0).getAsDouble(),v.get(1).getAsDouble(),v.get(2).getAsDouble()));
                if(row.has("fireTicks"))arrow.setRemainingFireTicks(row.get("fireTicks").getAsInt());
                if(row.has("sharedFire"))arrow.setSharedFlagOnFire(row.get("sharedFire").getAsBoolean());
                if(row.has("noPhysics"))arrow.setNoPhysics(row.get("noPhysics").getAsBoolean());
                if(row.has("noGravity"))arrow.setNoGravity(row.get("noGravity").getAsBoolean());
                if(row.has("inGround")) {
                    var setter=AbstractArrow.class.getDeclaredMethod("setInGround",boolean.class);setter.setAccessible(true);setter.invoke(arrow,row.get("inGround").getAsBoolean());
                    field(AbstractArrow.class,"life").setInt(arrow,1199);
                }
                var result=new ArrayList<String>();
                for(int tick=0;tick<row.get("ticks").getAsInt();tick++) {
                    arrow.setOldPosAndRot();arrow.tickCount++;arrow.tick();
                    result.add(bits(arrow.position())+"/"+bits(arrow.getDeltaMovement())+"/"+arrow.isInWater()+(row.has("sharedFire")?"/"+arrow.getRemainingFireTicks()+"/"+arrow.isOnFire():"")+(row.has("inGround")?"/"+arrow.isRemoved():""));
                }
                out.println(String.join(" ",result));
            }
        }
    }
}
