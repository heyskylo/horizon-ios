package dev.horizonlab.parity;

import java.util.*;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.core.Holder;
import net.minecraft.core.component.*;
import net.minecraft.sounds.*;
import net.minecraft.util.RandomSource;
import net.minecraft.world.*;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.animal.sheep.Sheep;
import net.minecraft.world.item.*;
import net.minecraft.world.level.Level;

/** Actual DyeItem interaction; sheep properties and sound delivery are fixture boundaries. */
public final class VanillaSheepDyeOracle {
    static Object unsafe;
    static java.lang.reflect.Method allocate;
    static World world;
    static List<String> heard = new ArrayList<>();

    static <T> T make(Class<T> type) throws Exception { return type.cast(allocate.invoke(unsafe, type)); }
    static void set(Object o, Class<?> c, String name, Object value) throws Exception {
        var f = c.getDeclaredField(name); f.setAccessible(true); f.set(o, value);
    }
    static class World extends ClientLevel {
        World() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }
        @Override public void playSeededSound(Entity except, Entity entity, Holder<SoundEvent> sound,
                SoundSource source, float volume, float pitch, long seed) {
            heard.add(sound.value().location()+":"+source.ordinal()+":"+volume+":"+pitch+":"+(except instanceof Actor && entity instanceof Subject));
        }
    }
    static class Actor extends LocalPlayer {
        Actor() { super(null, null, null, null, null, null, false, null, null); }
        @Override public Level level() { return world; }
    }
    static class Subject extends Sheep {
        DyeColor color; boolean alive, sheared;
        Subject() { super(null, null); }
        @Override public Level level() { return world; }
        @Override public boolean isAlive() { return alive; }
        @Override public boolean isSheared() { return sheared; }
        @Override public DyeColor getColor() { return color; }
        @Override public void setColor(DyeColor color) { this.color = color; }
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion(); net.minecraft.server.Bootstrap.bootStrap();
        var u = Class.forName("sun.misc.Unsafe"); var f = u.getDeclaredField("theUnsafe");
        f.setAccessible(true); unsafe = f.get(null); allocate = u.getMethod("allocateInstance", Class.class);
        Item[] items = {Items.DYE.white(), Items.DYE.red(), Items.DYE.black()};
        DyeColor[] defaults = {DyeColor.WHITE, DyeColor.RED, DyeColor.BLACK};
        for (int i = 0; i < items.length; i++) {
            items[i].builtInRegistryHolder().bindComponents(DataComponentMap.builder().set(DataComponents.DYE, defaults[i]).build());
        }
        world = make(World.class); var actor = make(Actor.class);
        try (var out = new java.io.PrintWriter(java.nio.file.Files.newBufferedWriter(java.nio.file.Path.of(args[0])))) {
            for (var initial : new DyeColor[]{DyeColor.WHITE, DyeColor.GRAY, DyeColor.RED, DyeColor.BLACK})
            for (var hand : InteractionHand.values()) for (var item : items) for (int patch = 0; patch < 3; patch++)
            for (boolean sheared : new boolean[]{false, true}) for (boolean alive : new boolean[]{false, true})
            for (boolean client : new boolean[]{false, true}) {
                set(world, Level.class, "isClientSide", client);
                set(world, Level.class, "soundSeedGenerator", RandomSource.create(42));
                var sheep = make(Subject.class); sheep.color = initial; sheep.sheared = sheared; sheep.alive = alive;
                var stack = new ItemStack(item, 2);
                if (patch == 1) stack.set(DataComponents.DYE, DyeColor.BLUE);
                if (patch == 2) stack.remove(DataComponents.DYE);
                for (int step = 0; step < 2; step++) {
                    heard.clear();
                    boolean result = ((DyeItem)item).interactLivingEntity(stack, actor, sheep, hand) == InteractionResult.SUCCESS;
                    out.println(initial.getId()+" "+hand+" "+item+" "+patch+" "+sheared+" "+alive+" "+client+" "+step+
                        " => result="+result+" color="+sheep.color.getId()+" count="+stack.getCount()+" sounds="+heard);
                }
            }
        }
        System.exit(0);
    }
}
