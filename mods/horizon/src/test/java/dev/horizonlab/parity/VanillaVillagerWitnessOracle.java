package dev.horizonlab.parity;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.Brain;
import net.minecraft.world.entity.ai.memory.NearestVisibleLivingEntities;
import net.minecraft.world.entity.ai.sensing.Sensing;
import net.minecraft.world.entity.npc.villager.Villager;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;

/** Actual lazy sensor visibility on isolated entities; no server is constructed. */
public final class VanillaVillagerWitnessOracle {
    static Object unsafe;
    static Method allocate;
    static Field field(Class<?> type, String name) throws Exception {
        var f = type.getDeclaredField(name);
        f.setAccessible(true);
        return f;
    }
    static <T> T make(Class<T> type) throws Exception {
        return type.cast(allocate.invoke(unsafe, type));
    }
    static final class Actor extends Villager {
        ServerLevel world;
        boolean invisible;
        Brain<Villager> brain;
        Sensing sensing;
        Actor() { super(EntityTypes.VILLAGER, null); }
        @Override public Level level() { return world; }
        @Override public boolean isAlive() { return true; }
        @Override public boolean isSpectator() { return false; }
        @Override public boolean isInvisible() { return invisible; }
        @Override public boolean isDiscrete() { return false; }
        @Override public float getArmorCoverPercentage() { return 0.0F; }
        @Override public ItemStack getItemBySlot(EquipmentSlot slot) { return ItemStack.EMPTY; }
        @Override public Brain<Villager> getBrain() { return brain; }
        @Override public Sensing getSensing() { return sensing; }
        @Override public boolean hasLineOfSight(Entity target) { return true; }
    }
    static Actor actor(ServerLevel world, int id, double x, double z, boolean invisible) throws Exception {
        var actor = make(Actor.class);
        actor.world = world;
        actor.invisible = invisible;
        actor.brain = new Brain<>();
        actor.sensing = new Sensing(actor);
        field(Entity.class, "id").setInt(actor, id);
        field(Entity.class, "position").set(actor, new Vec3(x, 1.0, z));
        return actor;
    }
    static String witnesses(NearestVisibleLivingEntities memory) {
        var ids = new ArrayList<Integer>();
        for (var entity : memory.findAll(e -> e instanceof Villager)) ids.add(entity.getId());
        return ids.toString().replace(" ", "");
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var u = Class.forName("sun.misc.Unsafe");
        unsafe = field(u, "theUnsafe").get(null);
        allocate = u.getMethod("allocateInstance", Class.class);
        var world = make(ServerLevel.class);
        var victim = actor(world, 1, 0, 0, false);
        var near = actor(world, 2, 10, 0, false);
        var hidden = actor(world, 3, 10, 0, true);
        var diagonal = actor(world, 4, 15, 15, false);
        var close = actor(world, 6, 1.5, 0, true);
        var far = actor(world, 7, 20, 0, false);
        List<LivingEntity> candidates = List.of(near, hidden, diagonal, close, far);
        var memory = new NearestVisibleLivingEntities(world, victim, candidates);
        try (var out = Files.newBufferedWriter(Path.of(args[0]))) {
            out.write("witnesses " + witnesses(memory) + "\n");
            near.invisible = true;
            hidden.invisible = false;
            out.write("cached " + witnesses(memory) + "\n");
            memory = new NearestVisibleLivingEntities(world, victim, candidates);
            out.write("fresh " + witnesses(memory) + "\n");
        }
    }
}
