package dev.horizonlab.parity;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundDisguisedChatPacket;
import net.minecraft.network.protocol.game.ClientboundLevelEventPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;

/**
 * The level events a client is sent (ClientboundLevelEventPacket: type, position, data, global), per server-chat
 * phase: the console says "level_events_NAME" to start a phase and "level_events_done" at the end; the client
 * answers each phase with "level events phase NAME".
 */
final class LevelEventTranscript implements PhasedTranscript {
    private final List<String> lines = new ArrayList<>();
    private String phase = "setup";
    private String pendingAck;
    private boolean done;

    @Override
    public void accept(Packet<?> packet) {
        String text = null;
        if (packet instanceof ClientboundSystemChatPacket p) text = p.content().getString();
        else if (packet instanceof ClientboundDisguisedChatPacket p) text = p.message().getString();
        if (text != null) {
            int at = text.lastIndexOf("level_events_");
            if (at >= 0) {
                String name = text.substring(at + "level_events_".length());
                if (name.equals("done")) done = true;
                else if (name.matches("[a-z_]+")) {
                    phase = name;
                    pendingAck = "level events phase " + name;
                }
            }
        }
        if (packet instanceof ClientboundLevelEventPacket p && !phase.equals("setup")) {
            lines.add(phase + " " + p.getType() + " " + p.getPos().toShortString().replace(", ", ",")
                + " " + p.getData() + " " + p.isGlobalEvent());
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
    @Override public String record() { return "play-level-event"; }
}
