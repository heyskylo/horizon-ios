package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.blaze3d.platform.InputConstants;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.client.MouseHandler;
import net.minecraft.client.Options;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.tutorial.Tutorial;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.phys.HitResult;

/**
 * The vanilla 26.3 client playing a tick-indexed script, for tools/test_wire_stream_parity.py: the same
 * script Horizon plays under HORIZON_TICK_SCRIPT, so the two clients' serverbound streams can be compared
 * tick by tick. It runs net.minecraft.client.main.Main unchanged; the script's lines act at a fixed point of
 * Minecraft.tick, after MultiPlayerGameMode.tick and the pick, before handleKeybinds (the client's Tutorial
 * is replaced by one whose onLookAt, called there every tick, plays the lines first). Key and mouse lines go
 * through KeyMapping as KeyboardHandler.keyPress and MouseHandler.onButton put them there, so everything
 * the keys do is vanilla's own code.
 *
 * stdin: "run PATH" plays the scenario in PATH (JSON {"name", "ticks", "steps": {"TICK": [LINE...]}}) from
 * the next tick: its first tick sends the command /wire-mark, which marks it in the capture. It prints
 * "[VanillaTickScript] done NAME" when the scenario's ticks have run. "where" prints the position.
 *
 * Lines (horizon-ctl's words): key NAME down|up|tap, mouse left|right down|up|click, look YAW PITCH,
 * click-slot SLOT [right] [shift] (the menu's slot id), command TEXT, close.
 */
public final class VanillaTickScript {
    private static final String TAG = "[VanillaTickScript] ";
    private static final Map<String, Integer> KEYS = new HashMap<>();

    static {
        // SDL scancodes (InputConstants' names), as the default key mappings use them.
        KEYS.put("w", InputConstants.KEY_W);
        KEYS.put("a", InputConstants.KEY_A);
        KEYS.put("s", InputConstants.KEY_S);
        KEYS.put("d", InputConstants.KEY_D);
        KEYS.put("space", InputConstants.KEY_SPACE);
        KEYS.put("shift", InputConstants.KEY_LSHIFT);
        KEYS.put("ctrl", InputConstants.KEY_LCONTROL);
        KEYS.put("e", InputConstants.KEY_E);
        KEYS.put("q", InputConstants.KEY_Q);
        KEYS.put("f", InputConstants.KEY_F);
        KEYS.put("esc", InputConstants.KEY_ESCAPE);
        for (int i = 1; i <= 9; i++) {
            KEYS.put(Integer.toString(i), InputConstants.KEY_1 + i - 1);
        }
    }

    private static volatile Script pending;
    private static Script running;
    private static int tick;

    private record Script(String name, int ticks, Map<Integer, List<String>> steps) {}

    public static void main(String[] args) throws Exception {
        Thread reader = new Thread(VanillaTickScript::read, "horizon-vanilla-tick-script");
        reader.setDaemon(true);
        reader.start();
        net.minecraft.client.main.Main.main(args);
    }

    private static void read() {
        try {
            Minecraft mc = waitForPlayer();
            mc.execute(() -> install(mc));
            BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
            for (String line; (line = in.readLine()) != null; ) {
                String[] words = line.trim().split("\\s+", 2);
                switch (words[0]) {
                    case "run" -> pending = load(Path.of(words[1]));
                    case "where" -> mc.execute(() -> System.out.println(TAG + "at " + mc.player.getX() + " "
                            + mc.player.getY() + " " + mc.player.getZ() + " " + mc.player.getYRot() + " "
                            + mc.player.getXRot()));
                    default -> System.out.println(TAG + "unknown: " + line);
                }
            }
        } catch (Throwable t) {
            t.printStackTrace();
        }
    }

    private static Script load(Path path) throws Exception {
        JsonObject json = JsonParser.parseString(Files.readString(path)).getAsJsonObject();
        Map<Integer, List<String>> steps = new HashMap<>();
        for (Map.Entry<String, JsonElement> e : json.getAsJsonObject("steps").entrySet()) {
            List<String> lines = new ArrayList<>();
            for (JsonElement l : (JsonArray) e.getValue()) {
                lines.add(l.getAsString());
            }
            steps.put(Integer.parseInt(e.getKey()), lines);
        }
        return new Script(json.get("name").getAsString(), json.get("ticks").getAsInt(), steps);
    }

    private static Minecraft waitForPlayer() throws InterruptedException {
        while (true) {
            Thread.sleep(250);
            Minecraft mc = Minecraft.getInstance();
            if (mc != null && mc.player != null && mc.level != null && mc.gui.screen() == null) {
                return mc;
            }
        }
    }

    /** Replace the client's Tutorial with one that plays the script; the mouse counts as grabbed. */
    private static void install(Minecraft mc) {
        try {
            Field tutorial = Minecraft.class.getDeclaredField("tutorial");
            tutorial.setAccessible(true);
            tutorial.set(mc, new Hook(mc, mc.options));
            grab(mc);
            System.out.println(TAG + "ready");
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    /**
     * MouseHandler.grabMouse's flag only: an unfocused window on a virtual screen never grabs, and
     * Minecraft.continueAttack and MouseHandler.onButton read it.
     */
    private static void grab(Minecraft mc) throws ReflectiveOperationException {
        Field grabbed = MouseHandler.class.getDeclaredField("mouseGrabbed");
        grabbed.setAccessible(true);
        grabbed.setBoolean(mc.mouseHandler, true);
    }

    private static final class Hook extends Tutorial {
        private final Minecraft mc;

        Hook(Minecraft mc, Options options) {
            super(mc, options);
            this.mc = mc;
        }

        @Override
        public void onLookAt(final ClientLevel level, final HitResult hit) {
            super.onLookAt(level, hit);
            step(mc);
        }
    }

    private static void step(Minecraft mc) {
        if (running == null) {
            if (pending == null) {
                return;
            }
            running = pending;
            pending = null;
            tick = 0;
            mc.player.connection.sendCommand("wire-mark");
        }
        try {
            grab(mc);
            for (String line : running.steps().getOrDefault(tick, List.of())) {
                apply(mc, line.trim().split("\\s+"));
            }
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
        tick++;
        if (tick > running.ticks()) {
            System.out.println(TAG + "done " + running.name());
            running = null;
        }
    }

    private static void apply(Minecraft mc, String[] w) throws ReflectiveOperationException {
        Screen screen = mc.gui.screen();
        switch (w[0]) {
            case "key" -> {
                InputConstants.Key key = InputConstants.Type.KEYBOARD.getOrCreate(KEYS.get(w[1]));
                boolean down = !w[2].equals("up");
                if (screen != null) {
                    // KeyboardHandler.keyPress hands the open screen the press: the inventory key and
                    // Escape close a container screen (AbstractContainerScreen.keyPressed, onClose).
                    if (down && (w[1].equals("e") || w[1].equals("esc"))) {
                        screen.onClose();
                    }
                    return;
                }
                if (down) {
                    KeyMapping.set(key, true);
                    KeyMapping.click(key);
                }
                if (!w[2].equals("down")) {
                    KeyMapping.set(key, false);
                }
            }
            case "mouse" -> {
                InputConstants.Key key = InputConstants.Type.MOUSE.getOrCreate(w[1].equals("left") ? InputConstants.MOUSE_BUTTON_LEFT : InputConstants.MOUSE_BUTTON_RIGHT);
                if (screen != null) {
                    return;
                }
                if (!w[2].equals("up")) {
                    KeyMapping.set(key, true);
                    KeyMapping.click(key);
                }
                if (!w[2].equals("down")) {
                    KeyMapping.set(key, false);
                }
            }
            case "look" -> {
                mc.player.setYRot(Float.parseFloat(w[1]));
                mc.player.setXRot(Float.parseFloat(w[2]));
                // A turn between frames comes before the tick's pick: pick again from the new view.
                Method pick = Minecraft.class.getDeclaredMethod("pick", float.class);
                pick.setAccessible(true);
                pick.invoke(mc, 1.0F);
            }
            case "click-slot" -> {
                int slot = Integer.parseInt(w[1]);
                boolean right = List.of(w).contains("right");
                boolean shift = List.of(w).contains("shift");
                mc.gameMode.handleContainerInput(mc.player.containerMenu.containerId, slot, right ? 1 : 0,
                        shift ? ContainerInput.QUICK_MOVE : ContainerInput.PICKUP, mc.player);
            }
            case "command" -> mc.player.connection.sendCommand(String.join(" ", List.of(w).subList(1, w.length)));
            case "close" -> {
                if (screen != null) {
                    screen.onClose();
                }
            }
            default -> System.out.println(TAG + "unknown line: " + String.join(" ", w));
        }
    }
}
