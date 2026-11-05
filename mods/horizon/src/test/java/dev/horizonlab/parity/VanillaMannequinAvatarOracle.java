package dev.horizonlab.parity;

import com.google.gson.*;
import java.lang.reflect.Field;
import java.nio.file.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.client.entity.ClientAvatarState;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.ai.attributes.*;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;
import sun.misc.Unsafe;

/** Executes the actual LivingEntity.tick, ElytraAnimationState and
 * ClientAvatarState methods. Only AI/movement, item use, attribute dirtiness
 * and world/flag getters are isolated; pose predicates and the counters,
 * interpolation and angle formulas remain native. No world simulation,
 * skin lookup or rendered-pixel parity is claimed. Frozen scenarios retain
 * Horizon's existing remote-level pause policy as a control. */
public final class VanillaMannequinAvatarOracle {
    private static final class Sensor extends LivingEntity {
        boolean flying;
        boolean alive;
        Pose pose;
        Vec3 velocity;
        Level client;
        AttributeMap attributes;
        // Instances are allocated without invoking this world-dependent constructor.
        Sensor() { super(null, null); }
        public HumanoidArm getMainArm() { return HumanoidArm.RIGHT; }
        public void baseTick() {}
        public void aiStep() {}
        public boolean isUsingItem() { return false; }
        public boolean isSleeping() { return false; }
        public Pose getPose() { return pose; }
        public boolean isFallFlying() { return flying; }
        public boolean isDeadOrDying() { return !alive; }
        public boolean isAlive() { return alive; }
        public Vec3 getDeltaMovement() { return velocity; }
        public Level level() { return client; }
        public AttributeMap getAttributes() { return attributes; }
        protected void tickHeadTurn(float target) {}
    }
    private static Field field(Class<?> owner, String name) throws Exception {
        Field result = owner.getDeclaredField(name);
        result.setAccessible(true);
        return result;
    }
    private static double number(Object state, Class<?> owner, String name) throws Exception {
        return ((Number) field(owner, name).get(state)).doubleValue();
    }
    private static JsonArray cloak(ClientAvatarState state, boolean old) throws Exception {
        JsonArray result = new JsonArray();
        for (String axis : List.of("x", "y", "z")) {
            result.add(number(state, ClientAvatarState.class, axis + "Cloak" + (old ? "O" : "")));
        }
        return result;
    }
    private static long bits(Object state, Class<?> owner, String name) throws Exception {
        return Integer.toUnsignedLong(Float.floatToRawIntBits((float) number(state, owner, name)));
    }
    private static JsonArray wingBits(ElytraAnimationState wings, boolean old) throws Exception {
        JsonArray result = new JsonArray();
        for (String axis : List.of("X", "Y", "Z")) {
            result.add(bits(wings, ElytraAnimationState.class, "rot" + axis + (old ? "Old" : "")));
        }
        return result;
    }
    private static Vec3 vector(JsonArray array) {
        return new Vec3(array.get(0).getAsDouble(), array.get(1).getAsDouble(), array.get(2).getAsDouble());
    }
    private static boolean flag(JsonObject input, String key) {
        return input.has(key) && input.get(key).getAsBoolean();
    }
    private static Sensor sensor(Unsafe unsafe, boolean alive) throws Exception {
        Sensor result = (Sensor) unsafe.allocateInstance(Sensor.class);
        result.client = (ClientLevel) unsafe.allocateInstance(ClientLevel.class);
        field(Level.class, "isClientSide").setBoolean(result.client, true);
        result.attributes = new AttributeMap(AttributeSupplier.builder().build());
        result.alive = alive;
        result.pose = Pose.STANDING;
        result.velocity = Vec3.ZERO;
        field(Entity.class, "position").set(result, Vec3.ZERO);
        field(LivingEntity.class, "swingState").set(result, new LivingEntity.SwingState());
        field(LivingEntity.class, "elytraAnimationState").set(result, new ElytraAnimationState(result));
        return result;
    }
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Unsafe unsafe = (Unsafe) field(Unsafe.class, "theUnsafe").get(null);
        Gson gson = new GsonBuilder().disableHtmlEscaping().create();
        JsonArray scenarios = JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray();
        List<String> lines = new ArrayList<>();
        for (JsonElement element : scenarios) {
            JsonObject scenario = element.getAsJsonObject();
            ClientAvatarState state = new ClientAvatarState();
            boolean player = flag(scenario, "player");
            boolean running = flag(scenario, "running");
            boolean alive = flag(scenario, "alive");
            boolean ground = flag(scenario, "on_ground");
            Sensor living = sensor(unsafe, alive);
            Vec3 previous = Vec3.ZERO;
            int step = 0;
            for (JsonElement row : scenario.getAsJsonArray("motions")) {
                JsonObject motion = row.getAsJsonObject();
                Vec3 position = vector(motion.getAsJsonArray("position"));
                Vec3 velocity = vector(motion.getAsJsonArray("velocity"));
                living.velocity = velocity;
                living.pose = motion.has("pose") ? Pose.values()[motion.get("pose").getAsInt()] : Pose.STANDING;
                living.flying = flag(motion, "flying");
                field(Entity.class, "position").set(living, position);
                if (player) {
                    // AbstractClientPlayer.tick: its avatar state precedes the base move.
                    state.tick(previous, velocity);
                    living.tick();
                    // AbstractClientPlayer.updateBob supplies this target.
                    float target = ground && alive && living.pose != Pose.SWIMMING
                        ? Math.min(0.1F, (float) velocity.horizontalDistance()) : 0.0F;
                    state.updateBob(target);
                } else if (running) {
                    // ClientMannequin.tick calls the base tick even while dead.
                    living.tick();
                    state.tick(position, velocity);
                }
                JsonObject result = new JsonObject();
                result.addProperty("case", scenario.get("name").getAsString());
                result.addProperty("step", step++);
                result.add("cloak", cloak(state, false));
                result.add("old", cloak(state, true));
                result.addProperty("bob_bits", bits(state, ClientAvatarState.class, "bob"));
                result.addProperty("bob_old_bits", bits(state, ClientAvatarState.class, "bobO"));
                result.addProperty("walk_bits", bits(state, ClientAvatarState.class, "walkDist"));
                result.addProperty("walk_old_bits", bits(state, ClientAvatarState.class, "walkDistO"));
                result.add("wing_bits", wingBits(living.elytraAnimationState, false));
                result.add("wing_old_bits", wingBits(living.elytraAnimationState, true));
                JsonArray half = new JsonArray();
                half.add(Integer.toUnsignedLong(Float.floatToRawIntBits(living.elytraAnimationState.getRotX(0.5F))));
                half.add(Integer.toUnsignedLong(Float.floatToRawIntBits(living.elytraAnimationState.getRotY(0.5F))));
                half.add(Integer.toUnsignedLong(Float.floatToRawIntBits(living.elytraAnimationState.getRotZ(0.5F))));
                result.add("wing_half_bits", half);
                result.addProperty("fall_fly_ticks", living.getFallFlyingTicks());
                result.addProperty("visual_crouching", living.isCrouching());
                result.addProperty("swim_bits", bits(living, LivingEntity.class, "swimAmount"));
                result.addProperty("swim_old_bits", bits(living, LivingEntity.class, "swimAmountO"));
                result.addProperty("swim_half_bits", Float.floatToRawIntBits(living.getSwimAmount(0.5F)));
                lines.add(gson.toJson(result));
                previous = position;
            }
        }
        Files.write(Path.of(args[1]), lines);
    }
}
