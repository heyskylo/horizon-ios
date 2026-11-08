package dev.horizonlab.parity;
import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.storage.loot.*;
import net.minecraft.world.level.storage.loot.predicates.*;
public class VanillaNamedRandomChanceOracle {
 public static void main(String[] args) throws Exception {
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  var ctor=LootContext.class.getDeclaredConstructors()[0];ctor.setAccessible(true);
  String z="{\"type\":\"minecraft:random_chance\",\"chance\":0}";
  String o="{\"type\":\"minecraft:random_chance\",\"chance\":1}";
  String half="{\"type\":\"minecraft:random_chance\",\"chance\":0.5}";
  String[] inputs={z,o,half,"{\"type\":\"minecraft:random_chance\",\"chance\":0.1}","{\"type\":\"minecraft:random_chance\",\"chance\":0.73087817}","{\"type\":\"minecraft:all_of\",\"terms\":["+z+","+half+"]}","{\"type\":\"minecraft:any_of\",\"terms\":["+o+","+half+"]}","{\"type\":\"minecraft:all_of\",\"terms\":["+o+","+half+"]}","{\"type\":\"minecraft:any_of\",\"terms\":["+z+","+half+"]}","{\"type\":\"minecraft:inverted\",\"term\":"+half+"}","{\"type\":\"minecraft:random_chance\",\"chance\":0.73087818}"};
  Gson gson=new Gson();
  for(long seed:new long[]{1,122,-1})for(int n=0;n<inputs.length;n++){
   var input=JsonParser.parseString(inputs[n]);var condition=LootItemCondition.DIRECT_CODEC.parse(JsonOps.INSTANCE,input).getOrThrow();
   var random=RandomSource.create(seed);var ctx=(LootContext)ctor.newInstance(null,random,null);boolean result=condition.test(ctx);
   JsonObject row=new JsonObject();row.addProperty("seed",seed);row.addProperty("case",n);row.add("input",input);row.addProperty("result",result);JsonArray next=new JsonArray();for(int i=0;i<4;i++)next.add(random.nextLong());row.add("continuation",next);System.out.println("chance-oracle "+gson.toJson(row));
  }
 }
}
