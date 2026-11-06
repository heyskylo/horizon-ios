package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.List;
import java.util.Optional;
import net.minecraft.core.component.DataComponents;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.ItemStack;
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
 * `assemble`. Fixture lines are "input-snbt reagent-snbt candidate-snbt".
 */
public final class VanillaBrewingOracle {
    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var recipes = new RecipeManager(ParityData.lookup);
        recipes.finalizeRecipeLoading(FeatureFlags.VANILLA_SET);
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
}
