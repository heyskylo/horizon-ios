package dev.horizonlab.parity;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.Entity;

/** Actual entity counter/contact methods on unconstructed instances. No world/server starts. */
public final class VanillaPortalCooldownOracle {
    public static void main(String[] args) throws Exception {
        // Registry initialization only; no MinecraftServer, level, services or EULA.
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var type = Class.forName("sun.misc.Unsafe");
        var field = type.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        var unsafe = field.get(null);
        var allocate = type.getMethod("allocateInstance", Class.class);
        Method tick = Entity.class.getDeclaredMethod("processPortalCooldown");
        tick.setAccessible(true);
        String[] classes = {
            "net.minecraft.world.entity.vehicle.boat.Boat",
            "net.minecraft.world.entity.vehicle.minecart.Minecart",
            "net.minecraft.world.entity.projectile.arrow.Arrow",
            "net.minecraft.world.entity.animal.pig.Pig"};
        List<Entity> entities = new ArrayList<>();
        List<Integer> delays = new ArrayList<>();
        for (String name : classes) {
            Entity entity = (Entity) allocate.invoke(unsafe, Class.forName(name));
            var passengers = Entity.class.getDeclaredField("passengers");
            passengers.setAccessible(true);
            passengers.set(entity, com.google.common.collect.ImmutableList.of());
            entity.setPortalCooldown();
            delays.add(entity.getPortalCooldown());
            entity.setPortalCooldown(5);
            entities.add(entity);
        }
        List<List<Integer>> rows = new ArrayList<>();
        int[] steps = {1, 3, 1, 1, 8};
        for (int step = 0; step < steps.length; step++) {
            for (int n = 0; n < steps[step]; n++) {
                for (int i = 0; i < entities.size(); i++) {
                    Entity entity = entities.get(i);
                    // AbstractArrow applies block effects before Entity.tick;
                    // the other three actors call Entity.tick before block effects.
                    if (step < 2 && i == 2) entity.setAsInsidePortal(null, BlockPos.ZERO);
                    tick.invoke(entity);
                    if (step < 2 && i != 2) entity.setAsInsidePortal(null, BlockPos.ZERO);
                }
            }
            rows.add(entities.stream().map(Entity::getPortalCooldown).toList());
        }
        Files.writeString(Path.of(args[0]), new com.google.gson.Gson().toJson(java.util.Map.of("delays", delays, "contact", rows)));
        System.out.println("Actual Entity counter/contact methods: 4 classes, 20 observations; no world/server");
    }
}
