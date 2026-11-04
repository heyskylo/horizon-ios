package dev.horizonlab.parity;

import java.util.List;
import net.minecraft.network.protocol.Packet;

/**
 * A scene the harness's console drives in phases (GoatVoiceTranscript, ElderCurseTranscript): the client asks
 * for it with {@link #request()} once its chunks are in, answers each phase barrier in chat with
 * {@link #acknowledgement()}, and records {@link #lines()} under {@link #record()} once it is {@link #done()}.
 */
interface PhasedTranscript {
    void accept(Packet<?> packet);

    String acknowledgement();

    boolean done();

    List<String> lines();

    /** The chat line that asks the console for the scene. */
    String request();

    /** The transcript's record type. */
    String record();
}
