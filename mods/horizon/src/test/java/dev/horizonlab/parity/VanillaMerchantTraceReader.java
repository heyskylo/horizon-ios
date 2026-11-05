package dev.horizonlab.parity;

import com.google.gson.*;
import io.netty.buffer.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.network.*;
import net.minecraft.network.protocol.*;
import net.minecraft.network.protocol.game.*;

/** Independently decode EVERY bounded raw receipt using the pinned official protocol.
 * Window IDs are retained; collecting a prior window never makes it the current one.
 * This checks encoding-state receipts, not network delivery or packet-family parity.
 */
public final class VanillaMerchantTraceReader {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var protocol=GameProtocols.CLIENTBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(ParityData.access));
        var codec=(net.minecraft.network.codec.StreamCodec<ByteBuf,Packet<?>>)(Object)protocol.codec();
        var rows=Files.readAllLines(Path.of(args[0]));
        JsonObject header=JsonParser.parseString(rows.getFirst()).getAsJsonObject();
        if(!header.get("event").getAsString().equals("start") || !header.get("uuid").getAsString().matches("[0-9a-f]{32}"))throw new IllegalStateException("missing actual login identity");
        boolean strictAll=args.length>2 && args[2].equals("--strict-all");
        int sequence=0;int nonidentical=0;int merchantFailures=0;int offerPackets=0;int contents=0;long elapsed=-1;boolean ended=false;
        try(var out=Files.newBufferedWriter(Path.of(args[1]))) {
            for(int n=0;n<rows.size();n++) {
                JsonObject row=JsonParser.parseString(rows.get(n)).getAsJsonObject();
                String event=row.get("event").getAsString();
                if(n==0) {out.write(row+"\n");continue;}
                if(event.equals("control")) {out.write(row+"\n");continue;}
                if(event.equals("end")) {
                    if(n!=rows.size()-1 || row.get("packets").getAsInt()!=sequence)throw new IllegalStateException("incomplete stream end");
                    if(row.has("channel_open") && !row.get("channel_open").getAsBoolean())throw new IllegalStateException("closed channel");
                    ended=true;out.write(row+"\n");continue;
                }
                if(!event.equals("packet") || row.get("sequence").getAsInt()!=sequence++)throw new IllegalStateException("packet sequence gap");
                long time=Long.parseLong(row.get("elapsed_ns").getAsString());if(time<elapsed)throw new IllegalStateException("time reversed");elapsed=time;
                byte[] payload=HexFormat.of().parseHex(row.get("data").getAsString());
                if(payload.length!=row.get("size").getAsInt())throw new IllegalStateException("truncated payload");
                var bytes=Unpooled.buffer();
                try {
                    new FriendlyByteBuf(bytes).writeVarInt(row.get("id").getAsInt());bytes.writeBytes(payload);
                    byte[] original=new byte[bytes.readableBytes()];bytes.getBytes(0,original);
                    Packet<?> packet=codec.decode(bytes);if(bytes.isReadable())throw new IllegalStateException("trailing bytes");
                    bytes.clear();codec.encode(bytes,packet);byte[] again=new byte[bytes.readableBytes()];bytes.readBytes(again);
                    boolean identical=Arrays.equals(original,again);
                    if(!identical) {nonidentical++;row.addProperty("reencoded_body",HexFormat.of().formatHex(again));}
                    row.addProperty("decoded_type",packet.type().toString());row.addProperty("decoded_class",packet.getClass().getName());row.addProperty("roundtrip",identical);
                    if(!identical) {
                        bytes.writeBytes(again);Packet<?> decodedAgain=codec.decode(bytes);
                        if(bytes.isReadable())throw new IllegalStateException("reencoded trailing bytes");
                        row.addProperty("decoded_again",decodedAgain.toString());
                        // Diagnostic only: Map/Set.equals ignore entry order. Other fields
                        // retain their own Java equality (including identity-only classes).
                        // This never replaces the strict original/again byte comparison.
                        JsonArray differences=new JsonArray();
                        for(var field:packet.getClass().getDeclaredFields()) {
                            if(java.lang.reflect.Modifier.isStatic(field.getModifiers()))continue;
                            field.setAccessible(true);Object before=field.get(packet),after=field.get(decodedAgain);
                            if(!Objects.deepEquals(before,after)) {
                                JsonObject diff=new JsonObject();diff.addProperty("field",field.getName());
                                diff.addProperty("before",String.valueOf(before));diff.addProperty("after",String.valueOf(after));
                                differences.add(diff);
                            }
                        }
                        row.add("unordered_field_differences",differences);
                        row.addProperty("field_comparison","Java deepEquals; maps/sets unordered; identity-only fields may differ");
                    }
                    boolean merchant=packet instanceof ClientboundMerchantOffersPacket || packet instanceof ClientboundContainerSetContentPacket || packet instanceof ClientboundContainerSetSlotPacket;
                    if(merchant && !identical)merchantFailures++;
                    if(packet instanceof ClientboundMerchantOffersPacket p) {
                        offerPackets++;
                        row.addProperty("window",p.getContainerId());row.addProperty("xp",p.getVillagerXp());
                        row.addProperty("level",p.getVillagerLevel());row.addProperty("can_restock",p.canRestock());row.addProperty("show_progress",p.showProgress());
                        JsonArray offers=new JsonArray();for(var o:p.getOffers()) {
                            JsonObject offer=new JsonObject();offer.addProperty("buy",ParityData.canon(o.getBaseCostA()));
                            offer.addProperty("cost",ParityData.canon(o.getCostA()));offer.addProperty("buyB",ParityData.canon(o.getCostB()));
                            offer.addProperty("sell",ParityData.canon(o.getResult()));offer.addProperty("uses",o.getUses());
                            offer.addProperty("maxUses",o.getMaxUses());offer.addProperty("xp",o.getXp());offer.addProperty("special",o.getSpecialPriceDiff());
                            offer.addProperty("demand",o.getDemand());offer.addProperty("multiplier",o.getPriceMultiplier());offers.add(offer);
                        }row.add("offers",offers);
                    } else if(packet instanceof ClientboundContainerSetContentPacket p) {
                        contents++;
                        row.addProperty("window",p.containerId());row.addProperty("revision",p.stateId());
                        JsonArray stacks=new JsonArray();for(var stack:p.items())stacks.add(ParityData.canon(stack));row.add("items",stacks);
                        row.addProperty("carried",ParityData.canon(p.carriedItem()));
                    } else if(packet instanceof ClientboundContainerSetSlotPacket p) {
                        row.addProperty("window",p.getContainerId());row.addProperty("revision",p.getStateId());
                        row.addProperty("slot",p.getSlot());row.addProperty("item",ParityData.canon(p.getItem()));
                    } else row.addProperty("decoded",packet.toString());
                    out.write(row+"\n");
                } finally {bytes.release();}
            }
        }
        if(!ended || sequence==0 || offerPackets==0 || contents==0)throw new IllegalStateException("missing/empty/inadmitted stream end");
        JsonObject summary=new JsonObject();summary.addProperty("complete_decode",true);summary.addProperty("packets",sequence);
        summary.addProperty("nonidentical_all_roundtrips",nonidentical);summary.addProperty("merchant_byte_failures",merchantFailures);
        summary.addProperty("offers_packets",offerPackets);summary.addProperty("content_packets",contents);summary.addProperty("strict_all",strictAll);
        Files.writeString(Path.of(args[1]+".summary.json"),summary.toString());
        System.out.println(sequence+" complete packets decoded and reencoded; "+nonidentical+" nonidentical roundtrips; login "+header.get("uuid"));
        if(merchantFailures!=0 || (strictAll && nonidentical!=0))System.exit(3);
    }
}
