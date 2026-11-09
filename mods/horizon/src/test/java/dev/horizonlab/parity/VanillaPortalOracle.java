package dev.horizonlab.parity;

import com.google.gson.*;
import java.io.PrintWriter;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.SectionPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.ai.village.poi.PoiManager;
import net.minecraft.world.entity.ai.village.poi.PoiSection;
import net.minecraft.world.entity.ai.village.poi.PoiTypes;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.NetherPortalBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.portal.PortalShape;

/**
 * Nether portal travel, one entity per case: `NetherPortalBlock.getPortalDestination`
 * for an entity standing in a portal of one headless level (the block update
 * oracle's), going to another (the Nether from the overworld or back), whose
 * points of interest are the case's portal blocks in the case's order. It runs
 * `PortalForcer.findClosestPortalPosition` and, when nothing is found,
 * `createPortal`, `BlockUtil.getLargestRectangleAround`,
 * `PortalShape.getRelativePosition` and `findCollisionFreePosition`. Printed:
 * the entity's size, the exit portal found (or "created"), the blocks the
 * destination changed and the arrival position (bits) and yaw. Cases with
 * kind "shape" instead print `PortalShape.findAnyShape` and
 * `findEmptyPortalShape` for a frame world, and the blocks `createPortalBlocks`
 * changes.
 */
public final class VanillaPortalOracle {
    /** The points of interest of the destination: the case's portal blocks. */
    static final class Poi extends PoiManager {
        Map<Long, PoiSection> sections;
        Poi() {
            super(null, null, null, false, null, null, null);
        }
        @Override protected Optional<PoiSection> getOrLoad(long key) { return Optional.ofNullable(sections.get(key)); }
        @Override public void ensureLoadedAndValid(net.minecraft.world.level.LevelReader reader, BlockPos center, int radius) { }
    }

    static net.minecraft.world.level.dimension.DimensionType OVERWORLD, NETHER;

    static String shapeLine(PortalShape shape) {
        try {
            var axisField = PortalShape.class.getDeclaredField("axis");
            var rightField = PortalShape.class.getDeclaredField("rightDir");
            var bottomField = PortalShape.class.getDeclaredField("bottomLeft");
            var widthField = PortalShape.class.getDeclaredField("width");
            var heightField = PortalShape.class.getDeclaredField("height");
            var blocksField = PortalShape.class.getDeclaredField("numPortalBlocks");
            for (var field : List.of(axisField, rightField, bottomField, widthField, heightField, blocksField)) {
                field.setAccessible(true);
            }
            var axis = (Direction.Axis) axisField.get(shape);
            var right = (Direction) rightField.get(shape);
            var bottom = (BlockPos) bottomField.get(shape);
            int width = widthField.getInt(shape);
            int height = heightField.getInt(shape);
            int blocks = blocksField.getInt(shape);
            return "axis=" + (axis == Direction.Axis.X ? "x" : "z") + " right=" + right.getName()
                + " bottom=" + bottom.getX() + "," + bottom.getY() + "," + bottom.getZ()
                + " size=" + width + "x" + height + " blocks=" + blocks
                + " valid=" + shape.isValid() + " complete=" + shape.isComplete();
        } catch (ReflectiveOperationException e) {
            throw new AssertionError(e);
        }
    }

    static List<String> runShape(JsonObject f) throws Exception {
        var copy = new JsonObject();
        copy.add("cells", f.get("source"));
        copy.add("seed", f.get("seed"));
        var w = VanillaBlockUpdateOracle.build(copy);
        var probe = f.getAsJsonObject("probe");
        BlockPos at = VanillaBlockUpdateOracle.readPos(probe.getAsJsonArray("pos"));
        Direction.Axis axis = probe.get("axis").getAsString().equals("x") ? Direction.Axis.X : Direction.Axis.Z;
        List<String> out = new ArrayList<>();
        out.add("shape " + shapeLine(PortalShape.findAnyShape(w, at, axis)));
        Map<BlockPos, BlockState> before = new HashMap<>(w.cells);
        var empty = PortalShape.findEmptyPortalShape(w, at, axis);
        if (empty.isEmpty()) {
            out.add("empty none");
        } else {
            out.add("empty " + shapeLine(empty.get()));
            empty.get().createPortalBlocks(w);
            out.addAll(VanillaBlockUpdateOracle.summary(w, before));
        }
        return out;
    }

    public static void main(String[] args) throws Exception {
        ParityData.init();
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        VanillaLevelShapeOracle.bindFluidTags(Path.of(args[0]).resolve("data/minecraft/tags/fluid"));
        OVERWORLD = VanillaBlockUpdateOracle.OVERWORLD_TYPE = VanillaBlockUpdateOracle.loadOverworld();
        NETHER = VanillaBlockUpdateOracle.loadDimensionType(BuiltinDimensionTypes.NETHER);
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[2]))); var lines = Files.lines(Path.of(args[1]))) {
            int index = 0;
            for (var line : (Iterable<String>) lines::iterator) {
                var f = JsonParser.parseString(line).getAsJsonObject();
                out.print("case " + index++ + "\n");
                try {
                    for (String s : run(f)) out.print(s + "\n");
                } catch (Throwable e) {
                    Throwable c = e;
                    while (c.getCause() != null) c = c.getCause();
                    if (System.getenv("ORACLE_TRACE") != null) c.printStackTrace();
                    StackTraceElement[] st = c.getStackTrace();
                    out.print("error " + c.getClass().getSimpleName() + " " + (st.length > 0 ? st[0].toString() : "") + " " + c.getMessage() + "\n");
                }
            }
        }
    }

    static VanillaBlockUpdateOracle.World level(JsonObject f, String cells, boolean nether) throws Exception {
        var copy = new JsonObject();
        copy.add("cells", f.get(cells));
        copy.add("seed", f.get("seed"));
        var w = VanillaBlockUpdateOracle.build(copy);
        w.type = nether ? NETHER : OVERWORLD;
        w.dimensionKey = nether ? Level.NETHER : Level.OVERWORLD;
        return w;
    }

    static List<String> run(JsonObject f) throws Exception {
        if (f.has("kind") && f.get("kind").getAsString().equals("shape")) return runShape(f);
        boolean fromNether = f.get("from").getAsString().equals("nether");
        var source = level(f, "source", fromNether);
        var target = level(f, "target", !fromNether);
        var levels = new HashMap<net.minecraft.resources.ResourceKey<Level>, net.minecraft.server.level.ServerLevel>();
        levels.put(source.dimension(), source);
        levels.put(target.dimension(), target);
        source.server.levels = levels;
        target.server = source.server;
        var border = new net.minecraft.world.level.border.WorldBorder();
        var b = f.getAsJsonObject("border");
        border.setCenter(b.get("x").getAsDouble(), b.get("z").getAsDouble());
        border.setSize(b.get("size").getAsDouble());
        target.border = border;
        var poi = (Poi) VanillaBlockUpdateOracle.allocate.invoke(VanillaBlockUpdateOracle.unsafe, Poi.class);
        poi.sections = new HashMap<>();
        VanillaBlockUpdateOracle.set(net.minecraft.world.level.chunk.storage.SectionStorage.class, poi, "levelHeightAccessor", target);
        var portalType = BuiltInRegistries.POINT_OF_INTEREST_TYPE.getOrThrow(PoiTypes.NETHER_PORTAL);
        for (var p : f.getAsJsonArray("pois")) {
            BlockPos at = VanillaBlockUpdateOracle.readPos(p.getAsJsonArray());
            poi.sections.computeIfAbsent(SectionPos.asLong(at), k -> new PoiSection(() -> { })).add(at, portalType);
        }
        target.poi = poi;
        VanillaBlockUpdateOracle.set(net.minecraft.server.level.ServerLevel.class, target, "portalForcer", new net.minecraft.world.level.portal.PortalForcer(target));
        var e = f.getAsJsonObject("entity");
        Entity entity = BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(e.get("type").getAsString())).create(source, EntitySpawnReason.LOAD);
        // Exact doubles, as bits.
        var at = e.getAsJsonArray("bits");
        java.util.function.IntToDoubleFunction bit = i -> Double.longBitsToDouble(Long.parseUnsignedLong(at.get(i).getAsString(), 16));
        entity.setPos(bit.applyAsDouble(0), bit.applyAsDouble(1), bit.applyAsDouble(2));
        List<String> out = new ArrayList<>();
        var dimensions = entity.getDimensions(entity.getPose());
        out.add(String.format("dims %08x %08x", Float.floatToIntBits(dimensions.width()), Float.floatToIntBits(dimensions.height())));
        Map<BlockPos, BlockState> before = new HashMap<>(target.cells);
        BlockPos entry = VanillaBlockUpdateOracle.readPos(f.getAsJsonArray("entry"));
        // The steps getPortalDestination takes, for the report.
        BlockState entryState = source.getBlockState(entry);
        if (entryState.hasProperty(NetherPortalBlock.AXIS)) {
            var axis = entryState.getValue(NetherPortalBlock.AXIS);
            var area = net.minecraft.util.BlockUtil.getLargestRectangleAround(entry, axis, 21, net.minecraft.core.Direction.Axis.Y, 21, q -> source.getBlockState(q) == entryState);
            out.add("entry " + VanillaBlockUpdateOracle.pos(area.minCorner) + " " + area.axis1Size + "x" + area.axis2Size);
            var offset = entity.getRelativePortalPosition(axis, area);
            out.add(String.format("offset %016x %016x %016x", Double.doubleToLongBits(offset.x), Double.doubleToLongBits(offset.y), Double.doubleToLongBits(offset.z)));
        }
        double scale = net.minecraft.world.level.dimension.DimensionType.getTeleportationScale(source.dimensionType(), target.dimensionType());
        BlockPos approximate = border.clampToBounds(entity.getX() * scale, entity.getY(), entity.getZ() * scale);
        out.add("approximate " + VanillaBlockUpdateOracle.pos(approximate));
        out.add("found " + target.getPortalForcer().findClosestPortalPosition(approximate, !fromNether, border).map(VanillaBlockUpdateOracle::pos).orElse("none"));
        var transition = ((NetherPortalBlock) Blocks.NETHER_PORTAL).getPortalDestination(source, entity, entry);
        out.addAll(VanillaBlockUpdateOracle.summary(target, before));
        if (transition == null) {
            out.add("dest none");
        } else {
            var p = transition.position();
            out.add(String.format("dest %016x %016x %016x %s", Double.doubleToLongBits(p.x), Double.doubleToLongBits(p.y), Double.doubleToLongBits(p.z),
                Float.toString(transition.yRot())));
        }
        return out;
    }
}
