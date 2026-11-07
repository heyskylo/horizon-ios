package dev.horizonlab.parity;

import com.mojang.authlib.GameProfile;
import java.io.PrintWriter;
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.util.debug.ServerDebugSubscribers;
import net.minecraft.world.Difficulty;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.material.FluidState;

/**
 * Original oracle: SpawnPlacements.isSpawnPositionOk and checkSpawnRules for an
 * entity type at a position of a controlled column world. The stand-in
 * ServerLevel answers block states from the case (air elsewhere), uniform sky
 * and block light, the sky darkening, thunder, difficulty, dimension type,
 * biome, sea level, WORLD_SURFACE height, world seed, the surface slime spawn
 * chance and an optional nearby player. Each case prints the two answers and
 * the next nextInt(1000) of its random, which pins the number of draws.
 */
public final class VanillaSpawnRulesOracle {
    static Object unsafe;
    static Method allocate, putObject, objectFieldOffset;

    static Object alloc(Class<?> c) throws Exception { return allocate.invoke(unsafe, c); }

    static void put(Object target, Class<?> owner, String field, Object value) throws Exception {
        Field f = owner.getDeclaredField(field);
        putObject.invoke(unsafe, target, (long) objectFieldOffset.invoke(unsafe, f), value);
    }

    static final class Lvl extends ServerLevel {
        Lvl() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        RegistryAccess registries;
        Map<Integer, BlockState> column = new HashMap<>();
        int x0, z0, sky, block, darken, sea, surface;
        boolean thunder;
        boolean unobstructed = true;
        boolean liquid;
        MinecraftServer server;
        long seed;
        Difficulty difficulty;
        DimensionType dimension;
        Holder<Biome> biome;
        net.minecraft.world.attribute.EnvironmentAttributeSystem attributes;
        List<Player> playerList = new ArrayList<>();
        WorldBorder border = new WorldBorder();
        int next = 1;
        @Override public RegistryAccess registryAccess() { return registries; }
        @Override public MinecraftServer getServer() { return server; }
        @Override public Difficulty getDifficulty() { return difficulty; }
        @Override public long getGameTime() { return 0L; }
        @Override public int getNextEntityId() { return next++; }
        @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() { return net.minecraft.world.flag.FeatureFlags.REGISTRY.allFlags(); }
        @Override public BlockState getBlockState(BlockPos pos) {
            if (pos.getX() != x0 || pos.getZ() != z0) return Blocks.AIR.defaultBlockState();
            return column.getOrDefault(pos.getY(), Blocks.AIR.defaultBlockState());
        }
        @Override public FluidState getFluidState(BlockPos pos) { return getBlockState(pos).getFluidState(); }
        @Override public boolean isUnobstructed(net.minecraft.world.entity.Entity entity) { return unobstructed; }
        @Override public boolean containsAnyLiquid(net.minecraft.world.phys.AABB box) { return liquid; }
        @Override public int getBrightness(LightLayer layer, BlockPos pos) { return layer == LightLayer.SKY ? sky : block; }
        @Override public int getRawBrightness(BlockPos pos, int darkening) { return Math.max(block, sky - darkening); }
        @Override public int getSkyDarken() { return darken; }
        @Override public boolean isThundering() { return thunder; }
        @Override public int getSeaLevel() { return sea; }
        @Override public int getHeight(Heightmap.Types type, int x, int z) { return surface; }
        @Override public long getSeed() { return seed; }
        @Override public DimensionType dimensionType() { return dimension; }
        @Override public Holder<Biome> getBiome(BlockPos pos) { return biome; }
        @Override public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() { return attributes; }
        @Override public WorldBorder getWorldBorder() { return border; }
        @SuppressWarnings({"unchecked", "rawtypes"})
        @Override public List players() { return playerList; }
    }

    static final class OraclePlayer extends Player {
        final GameType mode;
        OraclePlayer(Level level, GameType mode) { super(level, new GameProfile(new UUID(1L, 2L), "oracle")); this.mode = mode; }
        @Override public GameType gameMode() { return mode; }
        @Override public boolean isCreative() { return mode == GameType.CREATIVE; }
        @Override public boolean isSpectator() { return mode == GameType.SPECTATOR; }
        @Override public net.minecraft.world.item.component.ResolvableProfile getProfile() { return net.minecraft.world.item.component.ResolvableProfile.createResolved(this.getGameProfile()); }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        unsafe = unsafeField.get(null);
        allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
        putObject = unsafe.getClass().getMethod("putObject", Object.class, long.class, Object.class);
        objectFieldOffset = unsafe.getClass().getMethod("objectFieldOffset", Field.class);
        var repo = ServerPacksSource.createVanillaTrustedRepository();
        repo.reload();
        repo.setSelected(List.of("vanilla"));
        var resources = new MultiPackResourceManager(PackType.SERVER_DATA, repo.openAllSelected());
        var layers = RegistryLayer.createRegistryAccess();
        var staticTags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
        var context = TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, context, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        staticTags.forEach(Registry.PendingTags::apply);
        var access = layers.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();

        Lvl level = (Lvl) alloc(Lvl.class);
        level.registries = access;
        level.server = (MinecraftServer) alloc(VanillaBlockUpdateOracle.Server.class);
        put(level.server, MinecraftServer.class, "debugSubscribers", new ServerDebugSubscribers(level.server));
        level.column = new HashMap<>();
        level.playerList = new ArrayList<>();
        level.border = new WorldBorder();
        put(level, Level.class, "random", RandomSource.create(0L));
        var biomes = access.lookupOrThrow(Registries.BIOME);
        var dimensions = access.lookupOrThrow(Registries.DIMENSION_TYPE);
        var blocks = access.lookupOrThrow(Registries.BLOCK);

        if (args.length > 2 && args[2].equals("facts")) {
            try (var w = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
                for (var type : BuiltInRegistries.ENTITY_TYPE)
                    w.println(BuiltInRegistries.ENTITY_TYPE.getKey(type) + " peaceful=" + type.isAllowedInPeaceful() + " fire_immune=" + type.fireImmune()
                        + " placement=" + SpawnPlacements.getPlacementType(type) + " heightmap=" + SpawnPlacements.getHeightmapType(type));
            }
            System.exit(0);
        }

        List<String> lines = Files.readAllLines(Path.of(args[0]));
        try (var w = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (String raw : lines) {
                if (raw.isBlank()) continue;
                Map<String, String> c = new LinkedHashMap<>();
                for (String token : raw.trim().split("\\s+")) {
                    int eq = token.indexOf('=');
                    if (eq < 0) c.put("name", token); else c.put(token.substring(0, eq), token.substring(eq + 1));
                }
                try {
                    int y = Integer.parseInt(c.getOrDefault("y", "64"));
                    level.x0 = Integer.parseInt(c.getOrDefault("x", "8"));
                    level.z0 = Integer.parseInt(c.getOrDefault("z", "8"));
                    level.column.clear();
                    for (var e : c.entrySet()) {
                        if (e.getKey().startsWith("at")) {
                            int offset = Integer.parseInt(e.getKey().substring(2));
                            level.column.put(y + offset, net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(blocks, e.getValue(), false).blockState());
                        }
                    }
                    level.liquid = level.column.values().stream().anyMatch(state -> !state.getFluidState().isEmpty());
                    level.sky = Integer.parseInt(c.getOrDefault("sky", "15"));
                    level.block = Integer.parseInt(c.getOrDefault("light", "0"));
                    level.darken = Integer.parseInt(c.getOrDefault("darken", "0"));
                    level.thunder = c.getOrDefault("thunder", "0").equals("1");
                    level.sea = Integer.parseInt(c.getOrDefault("sea", "63"));
                    level.surface = Integer.parseInt(c.getOrDefault("surface", "0"));
                    level.seed = Long.parseLong(c.getOrDefault("wseed", "0"));
                    level.difficulty = Difficulty.byName(c.getOrDefault("difficulty", "normal"));
                    level.dimension = dimensions.getOrThrow(ResourceKey.create(Registries.DIMENSION_TYPE, Identifier.parse(c.getOrDefault("dim", "minecraft:overworld")))).value();
                    level.biome = biomes.getOrThrow(ResourceKey.create(Registries.BIOME, Identifier.parse(c.getOrDefault("biome", "minecraft:plains"))));
                    level.attributes = net.minecraft.world.attribute.EnvironmentAttributeSystem.builder()
                        .addConstantLayer(net.minecraft.world.attribute.EnvironmentAttributeMap.builder()
                            .set(net.minecraft.world.attribute.EnvironmentAttributes.SURFACE_SLIME_SPAWN_CHANCE, Float.parseFloat(c.getOrDefault("slime", "0.0")))
                            .build())
                        .build();
                    level.playerList.clear();
                    if (c.containsKey("player")) {
                        String[] p = c.get("player").split(",");
                        Player player = new OraclePlayer(level, GameType.byName(p.length > 3 ? p[3] : "survival"));
                        player.setPos(level.x0 + Double.parseDouble(p[0]), y + Double.parseDouble(p[1]), level.z0 + Double.parseDouble(p[2]));
                        level.playerList.add(player);
                    }
                    EntityType<?> type = BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(c.get("type")));
                    EntitySpawnReason reason = EntitySpawnReason.valueOf(c.getOrDefault("reason", "natural").toUpperCase(Locale.ROOT));
                    BlockPos pos = new BlockPos(level.x0, y, level.z0);
                    if (c.getOrDefault("mode", "").equals("obstruction")) {
                        level.unobstructed = !c.getOrDefault("unobstructed", "1").equals("0");
                        var entity = type.create(level, EntitySpawnReason.LOAD);
                        if (entity != null) entity.setPosRaw(level.x0 + 0.5, y, level.z0 + 0.5);
                        boolean obstruction = entity instanceof Mob mob && mob.checkSpawnObstruction(level);
                        w.println(c.get("name") + " " + obstruction);
                    } else {
                        RandomSource random = RandomSource.create(Long.parseLong(c.getOrDefault("seed", "0")));
                        boolean ok = SpawnPlacements.isSpawnPositionOk(type, level, pos);
                        boolean rules = SpawnPlacements.checkSpawnRules(type, level, reason, pos, random);
                        w.println(c.get("name") + " " + ok + " " + rules + " " + random.nextInt(1000));
                    }
                } catch (Throwable t) {
                    Throwable cause = t instanceof InvocationTargetException ite ? ite.getCause() : t;
                    StringBuilder line = new StringBuilder(c.get("name") + " error " + cause);
                    if (System.getenv("ORACLE_TRACE") != null) for (var frame : cause.getStackTrace()) line.append("\n  at ").append(frame);
                    w.println(line);
                }
            }
        }
        System.exit(0);
    }
}
