package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import net.minecraft.network.syncher.SynchedEntityData;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.raid.Raider;

/** Original oracle: real Raider setters and SynchedEntityData spawn/dirty values. */
public final class VanillaRaiderCelebrationOracle {
    static Field field(Class<?> owner, String name) throws Exception {
        var field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

    static Method definition(Class<?> owner) throws Exception {
        for (var type = owner; type != null; type = type.getSuperclass()) {
            try {
                var method = type.getDeclaredMethod("defineSynchedData", SynchedEntityData.Builder.class);
                method.setAccessible(true);
                return method;
            } catch (NoSuchMethodException ignored) { }
        }
        throw new NoSuchMethodException("defineSynchedData");
    }

    @SuppressWarnings("unchecked")
    static <T> void base(SynchedEntityData.Builder builder, String name, T value) throws Exception {
        builder.define((net.minecraft.network.syncher.EntityDataAccessor<T>) field(Entity.class, name).get(null), value);
    }

    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var unsafeClass = Class.forName("sun.misc.Unsafe");
        var unsafe = field(unsafeClass, "theUnsafe").get(null);
        var allocate = unsafeClass.getMethod("allocateInstance", Class.class);
        var entityData = field(Entity.class, "entityData");
        var accessor = (net.minecraft.network.syncher.EntityDataAccessor<?>) field(Raider.class, "IS_CELEBRATING").get(null);
        if (accessor.id() != 16 || accessor.serializer() != net.minecraft.network.syncher.EntityDataSerializers.BOOLEAN) {
            throw new AssertionError("Unexpected pinned raider metadata contract");
        }
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                var parts = line.split(" ");
                String name = switch (parts[0]) {
                    case "pillager" -> "illager.Pillager";
                    case "vindicator" -> "illager.Vindicator";
                    case "evoker" -> "illager.Evoker";
                    case "illusioner" -> "illager.Illusioner";
                    case "witch" -> "Witch";
                    case "ravager" -> "Ravager";
                    default -> throw new IllegalArgumentException(parts[0]);
                };
                var type = Class.forName("net.minecraft.world.entity.monster." + name);
                var raider = (Raider) allocate.invoke(unsafe, type);
                var builder = new SynchedEntityData.Builder(raider);
                // Entity's constructor defines its base entries before calling the
                // subclass method. Initialize those entries without constructing a level.
                base(builder, "DATA_SHARED_FLAGS_ID", (byte) 0);
                base(builder, "DATA_AIR_SUPPLY_ID", raider.getMaxAirSupply());
                base(builder, "DATA_CUSTOM_NAME", java.util.Optional.empty());
                base(builder, "DATA_CUSTOM_NAME_VISIBLE", false);
                base(builder, "DATA_SILENT", false);
                base(builder, "DATA_NO_GRAVITY", false);
                base(builder, "DATA_POSE", net.minecraft.world.entity.Pose.STANDING);
                base(builder, "DATA_TICKS_FROZEN", 0);
                definition(type).invoke(raider, builder);
                var data = builder.build();
                entityData.set(raider, data);
                var trace = new ArrayList<String>();
                for (int i = 1; i < parts.length; i++) {
                    if (!parts[i].equals("0") && !parts[i].equals("1")) throw new IllegalArgumentException(parts[i]);
                    raider.setCelebrating(parts[i].equals("1"));
                    var values = i == 1 ? data.getNonDefaultValues() : data.packDirty();
                    if (i == 1) data.packDirty();
                    var sent = new ArrayList<String>();
                    if (values != null) {
                        for (var value : values) {
                            if (value.id() == accessor.id()) sent.add((Boolean) value.value() ? "1" : "0");
                        }
                    }
                    trace.add((sent.isEmpty() ? "-" : String.join(",", sent)) + ":" + (raider.isCelebrating() ? "1" : "0"));
                }
                out.println(String.join(" ", trace));
            }
        }
    }
}
