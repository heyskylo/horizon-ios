package dev.horizonlab.parity;

import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.client.gui.Gui;
import net.minecraft.client.gui.screens.DeathScreen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.gui.screens.debug.DebugOptionsScreen;
import sun.misc.Unsafe;

/** Queries screen interruption without constructing Minecraft, a window or a server. */
public final class VanillaTitleDebugOracle {
    private static void set(Object target, String name, Object value) throws Exception {
        Field field = target.getClass().getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
    }

    public static void main(String[] args) throws Exception {
        Field singleton = Unsafe.class.getDeclaredField("theUnsafe");
        singleton.setAccessible(true);
        Unsafe unsafe = (Unsafe) singleton.get(null);
        TitleScreen title = (TitleScreen) unsafe.allocateInstance(TitleScreen.class);
        Gui gui = (Gui) unsafe.allocateInstance(Gui.class);
        StringBuilder result = new StringBuilder();
        result.append("title ").append(title.canInterruptWithAnotherScreen()).append('\n');
        set(gui, "screen", title);
        result.append("gui_title ").append(gui.canInterruptScreen()).append('\n');
        set(gui, "clientLevelTeardownInProgress", true);
        result.append("teardown ").append(gui.canInterruptScreen()).append('\n');
        set(gui, "clientLevelTeardownInProgress", false);
        set(gui, "screen", unsafe.allocateInstance(DeathScreen.class));
        result.append("death ").append(gui.canInterruptScreen()).append('\n');
        set(gui, "screen", unsafe.allocateInstance(DebugOptionsScreen.class));
        result.append("debug_options ").append(gui.canInterruptScreen()).append('\n');
        Files.writeString(Path.of(args[0]), result);
    }
}
