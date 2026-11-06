package dev.horizonlab.parity;

import net.minecraft.world.entity.*;
import com.google.gson.Gson;
import java.nio.file.*;
import java.io.PrintWriter;
import net.minecraft.world.entity.ai.Brain;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.core.component.DataComponentType;
import java.util.*;
public final class VanillaConversionTagsOracle {
/** Only tags and the actual conversion method are under test. Unrelated entity
 * state is inert, and allocation avoids constructing a world or starting a server. */
static class Probe extends Mob {
 Set<String> tags; Brain<?> brain;
 Probe(){super(null,null);}
 public float getAbsorptionAmount(){return 0;} public double getAttributeValue(net.minecraft.core.Holder<net.minecraft.world.entity.ai.attributes.Attribute> a){return 0;}
 public Collection<MobEffectInstance> getActiveEffects(){return List.of();}
 public boolean isBaby(){return false;} public Brain<?> getBrain(){return brain;}
 public boolean isLeftHanded(){return false;} public void setLeftHanded(boolean x){}
 public boolean isNoAi(){return false;} public void setNoAi(boolean x){}
 public boolean isPersistenceRequired(){return false;}
 public boolean isCustomNameVisible(){return false;} public void setCustomNameVisible(boolean x){}
 public boolean isOnFire(){return false;} public void setSharedFlagOnFire(boolean x){}
 public boolean isPermanentlyInvulnerable(){return false;} public void setPermanentlyInvulnerable(boolean x){}
 public int getInvulnerableTime(){return 0;} public void setInvulnerableTime(int x){}
 public boolean isNoGravity(){return false;} public void setNoGravity(boolean x){}
 public int getPortalCooldown(){return 0;} public void setPortalCooldown(int x){}
 public boolean isSilent(){return false;} public void setSilent(boolean x){}
 public Set<String> entityTags(){return tags;} public boolean addTag(String s){return tags.add(s);}
 public <T> T get(DataComponentType<? extends T> c){return null;}
}
public static void main(String[] args)throws Exception {
net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
var f=Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");f.setAccessible(true);var u=f.get(null);var a=u.getClass().getMethod("allocateInstance",Class.class);
var method=ConversionType.class.getDeclaredMethod("convertCommon",Mob.class,Mob.class,ConversionParams.class);
method.setAccessible(true);
var gson=new Gson();
try(var out=new PrintWriter(Files.newBufferedWriter(Path.of(args[1])))) {
 for(String line:Files.readAllLines(Path.of(args[0]))) {
  var from=(Probe)a.invoke(u,Probe.class);var to=(Probe)a.invoke(u,Probe.class);
  from.tags=new HashSet<>(Arrays.asList(gson.fromJson(line,String[].class)));
  to.tags=new HashSet<>();from.brain=new Brain<>();to.brain=new Brain<>();
  method.invoke(null,from,to,new ConversionParams(ConversionType.SINGLE,false,false,null));
  var result=new ArrayList<String>();
  for(String tag:to.entityTags()) {
   var encoded=new StringBuilder();
   for(char unit:tag.toCharArray()) encoded.append(String.format("%04x",(int)unit));
   result.add(encoded.toString());
  }
  Collections.sort(result);out.println(gson.toJson(result));
 }
}
}}
