package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.List;
import java.util.Locale;
import java.util.Optional;
import net.minecraft.core.BlockPos;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BrewingStandBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.item.crafting.BrewingInput;
import net.minecraft.world.item.crafting.BrewingRecipe;
import net.minecraft.world.item.crafting.PotionIngredient;
import net.minecraft.world.item.crafting.RecipeHolder;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.item.crafting.RecipePropertySet;
import net.minecraft.world.item.crafting.RecipeType;

/**
 * Original oracle for the brewing recipes: a real `RecipeManager` over the
 * vanilla data pack, its `brewing_input`/`brewing_reagent` property sets, the
 * three brewing-slot filters, and the first matching `BrewingRecipe`'s
 * `assemble`. Fixture lines are "input-snbt reagent-snbt candidate-snbt";
 * "tick" lines build a brewing stand in a block-update world and call the real
 * `BrewingStandBlockEntity.serverTick`, printing the slots, brew time, fuel,
 * speed, bottle bits, ingredient and brew-event count.
 */
public final class VanillaBrewingOracle {
    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        ParityData.init();
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        var allocator = Class.forName("sun.misc.Unsafe");
        var unsafeField = allocator.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = unsafeField.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        var recipes = new RecipeManager(ParityData.lookup);
        recipes.finalizeRecipeLoading(FeatureFlags.VANILLA_SET);
        VanillaBlockUpdateOracle.Server.recipes = recipes;
        List<RecipeHolder<BrewingRecipe>> brewing = recipes.getRecipes().stream()
                .filter(h -> h.value() instanceof BrewingRecipe)
                .map(h -> (RecipeHolder<BrewingRecipe>) h)
                .toList();
        var inputs = recipes.propertySet(RecipePropertySet.BREWING_INPUTS);
        var reagents = recipes.propertySet(RecipePropertySet.BREWING_REAGENTS);
        Path in = Path.of(args[0]), out = Path.of(args[1]);
        int n = 0;
        try (var w = new PrintWriter(Files.newBufferedWriter(out, StandardCharsets.UTF_8));
             var lines = Files.newBufferedReader(in, StandardCharsets.UTF_8)) {
            for (String line = lines.readLine(); line != null; line = lines.readLine()) {
                if (line.startsWith("#")) continue;
                String[] f = line.split(" ");
                if (f[0].equals("tick")) {
                    w.println(tick(n, f, recipes));
                    n++;
                    continue;
                }
                ItemStack input = ParityData.stack(f[0]);
                ItemStack reagent = ParityData.stack(f[1]);
                ItemStack candidate = ParityData.stack(f[2]);
                boolean potionSlot = !candidate.isEmpty() && PotionIngredient.isPotionInput(candidate, recipes);
                boolean reagentSlot = !candidate.isEmpty() && reagents.test(candidate);
                boolean fuelSlot = !candidate.isEmpty() && candidate.has(DataComponents.BREWING_FUEL);
                var brewingInput = new BrewingInput(input, reagent);
                Optional<RecipeHolder<BrewingRecipe>> found = brewing.stream()
                        .filter(h -> h.value().matches(brewingInput))
                        .findFirst();
                String result = found.map(h -> ParityData.canon(h.value().assemble(brewingInput))).orElse("-");
                String id = found.map(h -> h.id().identifier().toString()).orElse("-");
                w.println(n + " " + potionSlot + " " + reagentSlot + " " + fuelSlot + " " + result + " " + id);
                n++;
            }
        }
        System.exit(0);
    }

    private static Field field(Class<?> c, String name) throws Exception {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        return f;
    }

    private static void setInt(Object o, Class<?> c, String name, int v) throws Exception {
        field(c, name).setInt(o, v);
    }

    /** One brewing-stand tick run: initial slots/fields, `ticks` serverTick calls, final state. */
    private static String tick(int n, String[] f, RecipeManager recipes) throws Exception {
        int ticks = Integer.parseInt(f[1]);
        ItemStack[] slots = new ItemStack[5];
        for (int i = 0; i < 5; i++) slots[i] = ParityData.stack(f[2 + i]);
        int brewTime = Integer.parseInt(f[7]);
        int fuel = Integer.parseInt(f[8]);
        int totalBrew = Integer.parseInt(f[9]);
        int totalFuel = Integer.parseInt(f[10]);
        float speed = Float.parseFloat(f[11]);
        String ingredient = f[12];

        JsonObject caseJson = new JsonObject();
        JsonArray cells = new JsonArray();
        JsonArray cell = new JsonArray();
        cell.add(0);
        cell.add(0);
        cell.add(0);
        cell.add(Block.getId(Blocks.BREWING_STAND.defaultBlockState()));
        cells.add(cell);
        caseJson.add("cells", cells);
        caseJson.addProperty("seed", 0);
        VanillaBlockUpdateOracle.World w = VanillaBlockUpdateOracle.build(caseJson);
        w.registries = ParityData.access;
        var pos = new BlockPos(0, 0, 0);
        BlockState state = w.getBlockState(pos);
        var be = (BrewingStandBlockEntity) w.getBlockEntity(pos);
        for (int i = 0; i < 5; i++) be.setItem(i, slots[i]);
        setInt(be, BrewingStandBlockEntity.class, "brewTime", brewTime);
        setInt(be, BrewingStandBlockEntity.class, "fuel", fuel);
        setInt(be, BrewingStandBlockEntity.class, "totalBrewTime", totalBrew);
        setInt(be, BrewingStandBlockEntity.class, "totalFuel", totalFuel);
        field(BrewingStandBlockEntity.class, "speedMultiplier").setFloat(be, speed);
        Item ingredientItem = ingredient.equals("-") ? null
                : BuiltInRegistries.ITEM.getValue(Identifier.parse(ingredient));
        field(BrewingStandBlockEntity.class, "ingredient").set(be, ingredientItem);
        for (int t = 0; t < ticks; t++) {
            BrewingStandBlockEntity.serverTick(w, pos, state, be);
        }
        int brews = 0;
        for (int at = w.log.indexOf("L 1035 "); at >= 0; at = w.log.indexOf("L 1035 ", at + 1)) brews++;
        StringBuilder sb = new StringBuilder(Integer.toString(n));
        for (int i = 0; i < 5; i++) sb.append(" s").append(i).append('=').append(ParityData.canon(be.getItem(i)));
        sb.append(" time=").append(field(BrewingStandBlockEntity.class, "brewTime").getInt(be))
          .append(" fuel=").append(field(BrewingStandBlockEntity.class, "fuel").getInt(be))
          .append(" total=").append(field(BrewingStandBlockEntity.class, "totalBrewTime").getInt(be))
          .append(" totalfuel=").append(field(BrewingStandBlockEntity.class, "totalFuel").getInt(be))
          .append(" speed=").append(String.format(Locale.ROOT, "%.3f",
                  field(BrewingStandBlockEntity.class, "speedMultiplier").getFloat(be)))
          .append(" bits=");
        for (int i = 0; i < 3; i++) sb.append(be.getItem(i).isEmpty() ? '0' : '1');
        Item ing = (Item) field(BrewingStandBlockEntity.class, "ingredient").get(be);
        sb.append(" ing=").append(ing == null ? "-" : BuiltInRegistries.ITEM.getKey(ing).toString())
          .append(" brews=").append(brews);
        return sb.toString();
    }
}
