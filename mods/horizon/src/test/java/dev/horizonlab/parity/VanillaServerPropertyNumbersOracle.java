package dev.horizonlab.parity;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Properties;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.dedicated.DedicatedServerProperties;

/** Actual numeric settings conversion; constructs settings, never starts a server. */
public final class VanillaServerPropertyNumbersOracle {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        for (String value : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
            Properties raw = properties(value, "max-players", "op-permission-level", "max-tick-time");
            DedicatedServerProperties p = new DedicatedServerProperties(raw);
            System.out.println("RESULT " + p.maxPlayers.get() + " " + p.opPermissions.get().level().id()
                + " " + p.maxTickTime);

            Properties extra = properties(value, "difficulty", "gamemode", "max-world-size", "level-seed");
            DedicatedServerProperties q = new DedicatedServerProperties(extra);
            String seed = value.trim().isEmpty() ? "<random>" : Long.toString(q.worldOptions.seed());
            System.out.println("EXTRA " + q.difficulty.get().getId() + " " + q.gameMode.get().getId()
                + " " + q.maxWorldSize + " " + seed);

            Properties broadcast = properties(value, "entity-broadcast-range-percentage");
            try {
                System.out.println("BROADCAST "
                    + new DedicatedServerProperties(broadcast).entityBroadcastRangePercentage.get());
            } catch (NumberFormatException invalid) {
                System.out.println("BROADCAST ERROR");
            }
        }
    }

    private static Properties properties(String value, String... keys) {
        Properties result = new Properties();
        for (String key : keys) result.setProperty(key, value);
        return result;
    }
}
