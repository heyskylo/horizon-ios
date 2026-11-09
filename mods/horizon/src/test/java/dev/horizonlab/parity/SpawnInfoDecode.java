package dev.horizonlab.parity;
import java.nio.file.*;
import com.google.gson.*;
import io.netty.buffer.Unpooled;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.*;
public final class SpawnInfoDecode {
 public static void main(String[] args) throws Exception {
  ParityData.init();
  try(var out=Files.newBufferedWriter(Path.of(args[1]))) {
   for(String line:Files.readAllLines(Path.of(args[0]))) {
    var row=JsonParser.parseString(line).getAsJsonObject();
    if(!row.get("state").getAsString().equals("play") || !row.get("dir").getAsString().equals("s2c"))continue;
    String name=row.get("name").getAsString();
    if(!name.equals("login")&&!name.equals("respawn"))continue;
    var bytes=new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(java.util.HexFormat.of().parseHex(row.get("data").getAsString())),ParityData.access);
    try {
     CommonPlayerSpawnInfo s;
     if(name.equals("login"))s=ClientboundLoginPacket.STREAM_CODEC.decode(bytes).commonPlayerSpawnInfo();
     else s=ClientboundRespawnPacket.STREAM_CODEC.decode(bytes).commonPlayerSpawnInfo();
     if(bytes.isReadable())throw new AssertionError("trailing packet bytes");
     var j=new JsonObject();j.add("wire",row);
     j.addProperty("dimension_type",s.dimensionType().unwrapKey().orElseThrow().identifier().toString());
     j.addProperty("dimension",s.dimension().identifier().toString());j.addProperty("seed",s.seed());
     j.addProperty("mode",s.gameType().getId());j.addProperty("previous_mode",s.previousGameType().map(x->x.getId()).orElse(-1));
     j.addProperty("debug",s.isDebug());j.addProperty("flat",s.isFlat());
     j.addProperty("last_death",s.lastDeathLocation().toString());j.addProperty("portal_cooldown",s.portalCooldown());j.addProperty("sea_level",s.seaLevel());
     out.write(j.toString());out.newLine();
    }finally{bytes.release();}
   }
  }
 }
}
