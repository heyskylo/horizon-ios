package dev.horizonlab.parity;

import net.minecraft.core.BlockPos;
import io.netty.buffer.Unpooled;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundSetDefaultSpawnPositionPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.storage.LevelData;

/** Packet-data contract only: no server, level, world or player is constructed. */
public final class VanillaWorldSpawnPacketOracle {
    private static final java.io.PrintStream OUT = System.out;
    private static void emit(String label, String dimension, int x, int y, int z, float yaw, float pitch) {
        var data = LevelData.RespawnData.of(
            ResourceKey.create(Registries.DIMENSION, Identifier.parse(dimension)),
            new BlockPos(x, y, z), yaw, pitch);
        var buffer = new FriendlyByteBuf(Unpooled.buffer());
        ClientboundSetDefaultSpawnPositionPacket.STREAM_CODEC.encode(buffer,
            new ClientboundSetDefaultSpawnPositionPacket(data));
        var actual = ClientboundSetDefaultSpawnPositionPacket.STREAM_CODEC.decode(buffer).respawnData();
        buffer.release();
        OUT.println("world-spawn\t" + label + "\t" + actual.dimension().identifier()
            + "\t" + actual.pos().getX() + "\t" + actual.pos().getY() + "\t" + actual.pos().getZ()
            + ((label.equals("angle") || label.equals("respawn") || label.equals("normalized") || label.equals("clamped")) ? "\t" + actual.yaw() + "\t" + actual.pitch() : ""));
    }
    public static void main(String[] args) {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        emit("context", "minecraft:overworld", 100, 140, -20, 0.0F, 0.0F);
        emit("changed", "minecraft:overworld", 100, 140, -20, 0.0F, 0.0F);
        emit("override", "minecraft:the_end", 9, 70, -4, 0.0F, 0.0F);
        emit("angle", "minecraft:overworld", -7, 90, 12, 45.0F, -10.0F);
        emit("respawn", "minecraft:overworld", 0, 80, 0, 45.0F, -10.0F);
        emit("normalized", "minecraft:overworld", -7, 90, 12, 405.0F, 100.0F);
        emit("clamped", "minecraft:overworld", -7, 90, 12, 405.0F, -270.0F);
    }
}
