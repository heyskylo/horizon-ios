# Golden Hour (shader API 0.2)

[Golden Hour](../golden_hour) written for shader API 0.2 ([shaders-reference-0.2.md](../../../../docs/api/shaders-reference-0.2.md)):
the same sky and post passes in linear light. The glow and the golden tint work on light; the soft shoulder and the
saturation are a look written for display values, so the grade runs them on the encoded picture. It has no `[tonemap]`
of its own: Horizon's curve shows light above white, in SDR a clip at white like 0.1's clamp. 0.2 is experimental, so
this is a test pack of the SDK: the game draws it only with `HORIZON_HDR_FRAME=float` set, and the registry doesn't
list it. Check it with `horizon-shader check crates/horizon-shader-api/packs/golden_hour_0_2`.
