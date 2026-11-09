package dev.horizonlab.parity;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundAddEntityPacket;
import net.minecraft.network.protocol.game.ClientboundDisguisedChatPacket;
import net.minecraft.network.protocol.game.ClientboundLevelEventPacket;
import net.minecraft.network.protocol.game.ClientboundRemoveEntitiesPacket;
import net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;
import net.minecraft.network.syncher.EntityDataSerializers;
import net.minecraft.world.entity.EntityTypes;

/**
 * What a client is sent about the allays of a scene and the jukeboxes beside them, in arrival order, per
 * server-chat phase: each allay's spawn, every entity data value it is sent (ClientboundSetEntityDataPacket:
 * index, serializer and value, so Allay.DATA_DANCING among them) and its removal, named by the block it was
 * added in; and every level event (a jukebox's 1010 start and 1011 stop). The console says
 * "allay_dance_NAME" to start a phase and "allay_dance_done" at the end; the client answers each phase with
 * "allay dance phase NAME".
 */
final class AllayDanceTranscript implements PhasedTranscript {
    private final List<String> lines = new ArrayList<>();
    private final Map<Integer, String> allays = new HashMap<>();
    private String phase = "setup";
    private String pendingAck;
    private boolean done;

    @Override
    public void accept(Packet<?> packet) {
        String text = null;
        if (packet instanceof ClientboundSystemChatPacket p) text = p.content().getString();
        else if (packet instanceof ClientboundDisguisedChatPacket p) text = p.message().getString();
        if (text != null) {
            int at = text.lastIndexOf("allay_dance_");
            if (at >= 0) {
                String name = text.substring(at + "allay_dance_".length());
                if (name.equals("done")) done = true;
                else if (name.matches("[a-z0-9_]+")) {
                    phase = name;
                    pendingAck = "allay dance phase " + name;
                }
            }
        }
        if (packet instanceof ClientboundAddEntityPacket p && p.getType() == EntityTypes.ALLAY) {
            String name = "allay " + (int) Math.floor(p.getX()) + "," + (int) Math.floor(p.getY()) + ","
                + (int) Math.floor(p.getZ());
            allays.put(p.getId(), name);
            lines.add(phase + " | " + name + " | add");
        }
        if (packet instanceof ClientboundSetEntityDataPacket p && allays.containsKey(p.id())) {
            StringBuilder data = new StringBuilder();
            for (var v : p.packedItems()) {
                data.append(' ').append(v.id()).append('=').append(EntityDataSerializers.getSerializedId(v.serializer()))
                    .append(':').append(v.value());
            }
            lines.add(phase + " | " + allays.get(p.id()) + " | data" + data);
        }
        if (packet instanceof ClientboundRemoveEntitiesPacket p) {
            p.entityIds().forEach(id -> {
                if (allays.containsKey(id)) lines.add(phase + " | " + allays.get(id) + " | remove");
            });
        }
        if (packet instanceof ClientboundLevelEventPacket p) {
            lines.add(phase + " | level " + p.getType() + " " + p.getPos().toShortString().replace(", ", ",") + " "
                + p.getData() + " " + p.isGlobalEvent());
        }
    }

    @Override
    public String acknowledgement() {
        String ack = pendingAck;
        pendingAck = null;
        return ack;
    }

    @Override public boolean done() { return done; }
    @Override public List<String> lines() { return lines; }
    @Override public String request() { return "summon the scene"; }
    @Override public String record() { return "play-allay-dance"; }
}
