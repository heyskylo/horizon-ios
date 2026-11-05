package dev.horizonlab.parity;
import java.util.*;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.core.Holder;
import net.minecraft.core.component.DataComponentMap;
import net.minecraft.network.syncher.*;
import net.minecraft.sounds.*;
import net.minecraft.tags.ItemTags;
import net.minecraft.util.RandomSource;
import net.minecraft.world.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.ai.Brain;
import net.minecraft.world.entity.ai.memory.MemoryModuleType;
import net.minecraft.world.entity.animal.allay.Allay;
import net.minecraft.world.item.*;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;

/** Native client interaction-state oracle; entity construction and inventory insertion are fixture sinks. */
public final class VanillaAllayInteractionStateOracle {
 static Object unsafe;static java.lang.reflect.Method allocate;static World world;
 static java.lang.reflect.Constructor<?> dataCtor;static EntityDataAccessor<Boolean> canDuplicate;
 static List<String> heard=new ArrayList<>();
 static <T>T make(Class<T> type)throws Exception{return type.cast(allocate.invoke(unsafe,type));}
 static void set(Object o,Class<?> c,String name,Object value)throws Exception{var f=c.getDeclaredField(name);f.setAccessible(true);f.set(o,value);}
 static class World extends ClientLevel {
  World(){super(null,null,null,null,0,0,null,false,0L,0);}
  @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures(){return net.minecraft.world.flag.FeatureFlags.VANILLA_SET;}
  @Override public void playSeededSound(Entity except,Entity entity,Holder<SoundEvent> sound,SoundSource source,float volume,float pitch,long seed){heard.add(sound.value().location()+":"+source.ordinal()+":"+volume+":"+pitch+":"+(except instanceof net.minecraft.world.entity.player.Player));}
 }
 static class Actor extends LocalPlayer {
  ItemStack offered;boolean creative;List<String> returned;
  Actor(){super(null,null,null,null,null,null,false,null,null);}
  @Override public ItemStack getItemInHand(InteractionHand hand){return offered;}
  @Override public boolean hasInfiniteMaterials(){return creative;}
  @Override public UUID getUUID(){return new UUID(0,11);}
  @Override public boolean addItem(ItemStack stack){returned.add(stack(stack));return true;}
 }
 static class Subject extends Allay {
  ItemStack held;boolean dancing;Brain<Allay> brain;SimpleContainer inventory;
  Subject(){super(null,null);}
  @Override public Level level(){return world;}
  @Override public boolean isDancing(){return dancing;}
  @Override public ItemStack getItemInHand(InteractionHand hand){return held;}
  @Override public void setItemInHand(InteractionHand hand,ItemStack item){held=item;}
  @Override public void setItemSlot(EquipmentSlot slot,ItemStack item){if(slot==EquipmentSlot.MAINHAND)held=item;}
  @Override public void onSyncedDataUpdated(EntityDataAccessor<?> value){}
  @Override public Brain<Allay> getBrain(){return brain;}
  @Override public SimpleContainer getInventory(){return inventory;}
  @Override public void snapTo(Vec3 p){try{set(this,Entity.class,"position",p);}catch(Exception e){throw new RuntimeException(e);}}
  void interact(Actor actor,InteractionHand hand){super.mobInteract(actor,hand);}
 }
 static String stack(ItemStack s){return s.isEmpty()?"empty":s.getItem()+"x"+s.getCount();}
 static Subject subject(ItemStack held,boolean dancing,boolean can)throws Exception{
  var mob=make(Subject.class);mob.held=held;mob.dancing=dancing;mob.brain=new Brain<>();mob.inventory=new SimpleContainer(1);set(mob,LivingEntity.class,"activeEffects",new HashMap<>());set(mob,LivingEntity.class,"swingState",new LivingEntity.SwingState());
  var register=Brain.class.getDeclaredMethod("registerMemory",MemoryModuleType.class);register.setAccessible(true);register.invoke(mob.brain,MemoryModuleType.LIKED_PLAYER);
  var values=new SynchedEntityData.DataItem<?>[canDuplicate.id()+1];values[canDuplicate.id()]=new SynchedEntityData.DataItem<>(canDuplicate,can);
  set(mob,Entity.class,"entityData",dataCtor.newInstance(mob,values));set(mob,Entity.class,"position",new Vec3(.5,64,.5));return mob;
 }
 @SuppressWarnings("unchecked") public static void main(String[]args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var u=Class.forName("sun.misc.Unsafe");var f=u.getDeclaredField("theUnsafe");f.setAccessible(true);unsafe=f.get(null);allocate=u.getMethod("allocateInstance",Class.class);
  for(var item:new Item[]{Items.AIR,Items.STONE,Items.AMETHYST_SHARD})item.builtInRegistryHolder().bindComponents(DataComponentMap.EMPTY);
  world=make(World.class);set(world,Level.class,"isClientSide",true);dataCtor=SynchedEntityData.class.getDeclaredConstructors()[0];dataCtor.setAccessible(true);
  var can=Allay.class.getDeclaredField("DATA_CAN_DUPLICATE");can.setAccessible(true);canDuplicate=(EntityDataAccessor<Boolean>)can.get(null);
  EntityType.EntityFactory<Allay> factory=(type,level)->{try{return subject(ItemStack.EMPTY,false,true);}catch(Exception e){throw new RuntimeException(e);}};
  set(EntityTypes.ALLAY,EntityType.class,"factory",factory);
  var reference=Items.AMETHYST_SHARD.builtInRegistryHolder();
  var bind=Arrays.stream(reference.getClass().getDeclaredMethods()).filter(m->m.getName().equals("bindTags")).findFirst().orElseThrow();bind.setAccessible(true);
  try(var out=new java.io.PrintWriter(java.nio.file.Files.newBufferedWriter(java.nio.file.Path.of(args[0])))){
   for(int tagMode=0;tagMode<3;tagMode++)for(var hand:InteractionHand.values())for(var item:new Item[]{Items.AIR,Items.STONE,Items.AMETHYST_SHARD})for(boolean holding:new boolean[]{false,true})for(boolean dancing:new boolean[]{false,true})for(boolean duplicate:new boolean[]{false,true})for(boolean creative:new boolean[]{false,true}){
    bind.invoke(Items.AMETHYST_SHARD.builtInRegistryHolder(),tagMode==2?List.of():List.of(ItemTags.DUPLICATES_ALLAYS));
    bind.invoke(Items.STONE.builtInRegistryHolder(),tagMode==1?List.of(ItemTags.DUPLICATES_ALLAYS):List.of());
    var mob=subject(holding?new ItemStack(Items.STONE):ItemStack.EMPTY,dancing,duplicate);
    var actor=make(Actor.class);actor.offered=item==Items.AIR?ItemStack.EMPTY:new ItemStack(item);actor.creative=creative;actor.returned=new ArrayList<>();
    set(world,Level.class,"soundSeedGenerator",RandomSource.create(42));
    for(int step=0;step<3;step++){
     heard.clear();actor.returned.clear();mob.interact(actor,hand);
     out.println(tagMode+" "+hand+" "+item+" "+holding+" "+dancing+" "+duplicate+" "+creative+" "+step+" => held="+stack(mob.held)+" offer="+stack(actor.offered)+" can="+mob.getEntityData().get(canDuplicate)+" liked="+mob.brain.hasMemoryValue(MemoryModuleType.LIKED_PLAYER)+" returned="+actor.returned+" sounds="+heard);
    }
   }
  }
  System.exit(0);
 }
}
