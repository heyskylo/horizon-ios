package dev.horizonlab.parity;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundDisguisedChatPacket;
import net.minecraft.network.protocol.game.ClientboundGameEventPacket;
import net.minecraft.network.protocol.game.ClientboundSystemChatPacket;

/**
 * The elder guardians' curses on the player (ElderGuardian.customServerAiStep: GUARDIAN_ELDER_EFFECT, whose
 * parameter says whether ClientPacketListener.handleGameEvent plays the curse sound), per server-chat phase.
 */
final class ElderCurseTranscript implements PhasedTranscript {
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
            for (String name : List.of("loud", "silent")) {
                if (text.endsWith("elder_curse_" + name)) {
                    phase = name;
                    pendingAck = "elder curse phase " + name;
                }
            }
            if (text.endsWith("elder_curse_done")) done = true;
        }
        if (packet instanceof ClientboundGameEventPacket p
                && p.getEvent() == ClientboundGameEventPacket.GUARDIAN_ELDER_EFFECT) {
            lines.add(phase + " " + p.getParam());
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
    @Override public String request() { return "summon the elders"; }
    @Override public String record() { return "play-elder-curse"; }
}
