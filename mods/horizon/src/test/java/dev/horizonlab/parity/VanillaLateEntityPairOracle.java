package dev.horizonlab.parity;
import java.util.*;
import net.minecraft.core.component.DataComponentMap;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientGamePacketListener;
import net.minecraft.server.level.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.projectile.arrow.ThrownTrident;
import net.minecraft.world.item.Items;
import net.minecraft.world.phys.Vec3;

/** Real trident construction and ServerEntity pairing/updates; only the level ID allocator and packet delivery are fixture boundaries. */
public final class VanillaLateEntityPairOracle {
    static class World extends ServerLevel {
        World() { super(null,null,null,null,null,null,false,0L,List.of(),false); }
        @Override public int getNextEntityId() { return 37; }
    }
    static List<Packet<? super ClientGamePacketListener>> packets = new ArrayList<>();
    static class Sync implements ServerEntity.Synchronizer {
        public void sendToTrackingPlayers(Packet<? super ClientGamePacketListener> p) { packets.add(p); }
        public void sendToTrackingPlayersAndSelf(Packet<? super ClientGamePacketListener> p) { packets.add(p); }
        public void sendToTrackingPlayersFiltered(Packet<? super ClientGamePacketListener> p, java.util.function.Predicate<ServerPlayer> filter) { packets.add(p); }
    }
    static int ticks(ServerEntity s) throws Exception {
        var f = ServerEntity.class.getDeclaredField("tickCount"); f.setAccessible(true); return f.getInt(s);
    }
    static String packet(Packet<?> p) {
        var bytes = new net.minecraft.network.RegistryFriendlyByteBuf(io.netty.buffer.Unpooled.buffer(),
            net.minecraft.core.RegistryAccess.fromRegistryOfRegistries(net.minecraft.core.registries.BuiltInRegistries.REGISTRY));
        try {
            String name;
            if (p instanceof net.minecraft.network.protocol.game.ClientboundAddEntityPacket v) {
                name="add"; net.minecraft.network.protocol.game.ClientboundAddEntityPacket.STREAM_CODEC.encode(bytes,v);
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetEntityMotionPacket v) {
                name="motion"; net.minecraft.network.protocol.game.ClientboundSetEntityMotionPacket.STREAM_CODEC.encode(bytes,v);
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundMoveEntityPacket.PosRot v) {
                name="posrot"; net.minecraft.network.protocol.game.ClientboundMoveEntityPacket.PosRot.STREAM_CODEC.encode(bytes,v);
            } else if (p instanceof net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket v) {
                name="data"; net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket.STREAM_CODEC.encode(bytes,v);
            } else { throw new AssertionError(p); }
            byte[] payload = new byte[bytes.readableBytes()]; bytes.readBytes(payload);
            return name+":"+java.util.HexFormat.of().formatHex(payload);
        } finally { bytes.release(); }
    }
    static void dump(java.io.PrintWriter out, int index, String phase, ServerEntity s, int expectedTicks) throws Exception {
        if (ticks(s) != expectedTicks) throw new AssertionError("unexpected native update clock");
        out.println(index+" "+phase+" "+String.join(",",packets.stream().map(VanillaLateEntityPairOracle::packet).toList()));
        packets.clear();
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion(); net.minecraft.server.Bootstrap.bootStrap();
        var u=Class.forName("sun.misc.Unsafe");var f=u.getDeclaredField("theUnsafe");f.setAccessible(true);
        var unsafe=f.get(null); var level=(World)u.getMethod("allocateInstance",Class.class).invoke(unsafe,World.class);
        Items.TRIDENT.builtInRegistryHolder().bindComponents(DataComponentMap.EMPTY);
        var ground = net.minecraft.world.entity.projectile.arrow.AbstractArrow.class.getDeclaredMethod("setInGround",boolean.class);
        ground.setAccessible(true);
        try (var out = new java.io.PrintWriter(java.nio.file.Files.newBufferedWriter(java.nio.file.Path.of(args[0])))) {
            out.println("type "+net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getId(EntityTypes.TRIDENT));
            int index=0;
            for (var pos : new Vec3[]{new Vec3(.5,65.5,.5),new Vec3(-.123456789,64.125,-.5)})
            for (var velocity : new Vec3[]{new Vec3(.25,0,0),Vec3.ZERO,new Vec3(.25,-.5,.125)})
            for (boolean landed : new boolean[]{false,true}) {
                var t = new ThrownTrident(EntityTypes.TRIDENT, level);
                t.setId(37); t.setUUID(new UUID(0,37)); t.setPos(pos); t.setDeltaMovement(velocity);
                var tracker = new ServerEntity(level,t,UpdateInterval.periodic(20),true,new Sync());
                tracker.sendPairingData(null,packets::add); dump(out,index,"pair",tracker,0);
                t.setPos(pos.add(.25,0,0)); t.setDeltaMovement(Vec3.ZERO); ground.invoke(t,landed);
                tracker.sendChanges(); dump(out,index,"change",tracker,1);
                tracker.sendChanges(); dump(out,index,"again",tracker,2);
                index++;
            }
        }
        System.exit(0);
    }
}
