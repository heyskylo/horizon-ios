package dev.horizonlab.parity;

import com.google.common.collect.ImmutableList;
import com.google.gson.*;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.client.player.*;
import net.minecraft.core.*;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.vehicle.boat.Boat;
import net.minecraft.world.entity.vehicle.boat.AbstractBoat;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.*;
import net.minecraft.world.phys.*;

/** Actual boat authority, LocalPlayer rideTick and sendChanges; world lifecycle and packet sinks only are isolated. */
public final class VanillaBoatRiderOracle {
    static Field field(Class<?> type, String name) throws Exception {
        Field f=type.getDeclaredField(name); f.setAccessible(true); return f;
    }
    static <T> T make(Class<T> type) throws Exception {
        return type.cast(VanillaVehicleContainerOracle.allocate.invoke(VanillaVehicleContainerOracle.unsafe,type));
    }
    static class Lvl extends ServerLevel {
        int next, paddles;
        Lvl(){super(null,null,null,null,null,null,false,0L,List.of(),false);}
        @Override public boolean isClientSide(){return true;}
        @Override public int getNextEntityId(){return next++;}
        @Override public void sendPacketToServer(Packet<?> p){if(p instanceof ServerboundPaddleBoatPacket) paddles++;}
        @Override public RegistryAccess registryAccess(){return ParityData.access;}
        @Override public BlockState getBlockState(BlockPos p){return Blocks.AIR.defaultBlockState();}
        @Override public FluidState getFluidState(BlockPos p){return Fluids.EMPTY.defaultFluidState();}
        @Override public List<Entity> getEntities(Entity e,AABB b,Predicate<? super Entity> p){return List.of();}
    }
    static class Vessel extends Boat {
        Vessel(Lvl l){super(EntityTypes.OAK_BOAT,l,()->Items.OAK_BOAT);}
        @Override public void baseTick(){}
        @Override public void move(MoverType t,Vec3 m){}
        @Override protected void applyEffectsFromBlocks(){}
        @Override protected void positionRider(Entity e, Entity.MoveFunction f){}
        @Override public float getGroundFriction(){return 0;}
        @Override public net.minecraft.world.scores.PlayerTeam getTeam(){return null;}
    }
    static class Rider extends LocalPlayer {
        Rider(){super(null,null,null,null,null,null,false,null,null);}
        @Override public void tick(){}
        @Override protected void updateBob(){}
        @Override public boolean isSprinting(){return false;}
        @Override public boolean onGround(){return false;}
        @Override protected boolean isControlledCamera(){return true;}
    }
    static class Sink extends ClientPacketListener {
        int moves;
        Sink(){super(null,null,null);}
        @Override public boolean hasClientLoaded(){return true;}
        @Override public void send(Packet<?> p){if(p instanceof ServerboundMoveVehiclePacket)moves++;}
    }
    public static void main(String[] args)throws Exception {
        ParityData.init(); VanillaVehicleContainerOracle.reflect();
        Lvl l=make(Lvl.class); field(Level.class,"registryAccess").set(l,ParityData.access);
        JsonArray out=new JsonArray();
        for(var element:JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray()){
            JsonObject c=element.getAsJsonObject();
            Rider p=make(Rider.class); Sink sink=make(Sink.class);
            field(Entity.class,"level").set(p,l);
            field(Entity.class,"position").set(p,Vec3.ZERO);
            field(AbstractClientPlayer.class,"clientAvatarState").set(p,new net.minecraft.client.entity.ClientAvatarState());
            field(LocalPlayer.class,"connection").set(p,sink);
            field(LocalPlayer.class,"lastSentInput").set(p,Input.EMPTY);
            ClientInput input=new ClientInput();
            field(LocalPlayer.class,"input").set(p,input);
            JsonArray keys=c.getAsJsonArray("keys");
            input.keyPresses=new Input(keys.get(2).getAsBoolean(),keys.get(3).getAsBoolean(),keys.get(0).getAsBoolean(),keys.get(1).getAsBoolean(),false,false,false);
            Entity other=c.get("mob").getAsBoolean()?make(net.minecraft.world.entity.monster.zombie.Zombie.class):make(RemotePlayer.class); p.setId(1); other.setId(2);
            Vessel boat=new Vessel(l); boat.setPos(0,100,0);
            field(Entity.class,"vehicle").set(p,boat);
            JsonArray vel=c.getAsJsonArray("velocity");
            boat.setDeltaMovement(new Vec3(vel.get(0).getAsDouble(),vel.get(1).getAsDouble(),vel.get(2).getAsDouble()));
            Field spin=field(AbstractBoat.class,"deltaRotation");spin.setFloat(boat,c.get("spin").getAsFloat());
            JsonArray steps=new JsonArray();
            for(var own:c.getAsJsonArray("first")){
                boolean first=own.getAsBoolean();
                field(Entity.class,"passengers").set(boat,first?ImmutableList.of(p,other):ImmutableList.of(other,p));
                sink.moves=0;l.paddles=0;
                boat.tick();p.rideTick();p.sendChanges();
                if(c.get("event").getAsInt()!=0)boat.handleEntityEvent(c.get("event").getAsByte());
                JsonObject s=new JsonObject();
                s.addProperty("controlled",p.getControlledVehicle()==boat);
                s.addProperty("accepts_server_moves",!boat.isLocalInstanceAuthoritative());
                s.addProperty("hands_busy",p.isHandsBusy());
                s.addProperty("moves",sink.moves);s.addProperty("paddles",l.paddles);
                s.addProperty("spin",String.format("%08x",Float.floatToRawIntBits(spin.getFloat(boat))));
                JsonArray v=new JsonArray();for(double n:new double[]{boat.getDeltaMovement().x,boat.getDeltaMovement().y,boat.getDeltaMovement().z})v.add(String.format("%016x",Double.doubleToRawLongBits(n)));
                s.add("velocity",v);steps.add(s);
            }
            JsonObject row=new JsonObject();row.add("name",c.get("name"));row.add("steps",steps);out.add(row);
        }
        Files.writeString(Path.of(args[1]),out.toString());
    }
}
