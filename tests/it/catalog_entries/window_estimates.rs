//! What a model whose layers attend only to a window is charged for its
//! cache.
//!
//! Apart from the other estimates because what varies here is the model's
//! layers rather than the entry: each case writes one architecture's shape and
//! reads the same one-entry catalog against it.

use super::memory_estimates::estimated;
use crate::fixtures::{Gguf, Value};

/// A model of thirty layers, optionally declaring that most of them attend
/// only to a window rather than to the whole context.
///
/// The pattern marks a layer 1 when it slides and 0 when it attends fully,
/// five sliding to every one full, which is the shape Gemma ships.
fn windowed(sliding: bool) -> Gguf {
    let model = Gguf::model("gemma4", 30, 8192, 256)
        .with("gemma4.attention.head_count", Value::U32(4))
        .with("gemma4.attention.head_count_kv", Value::U32s(vec![2; 30]))
        .with("gemma4.attention.key_length", Value::U32(128))
        .with("gemma4.attention.value_length", Value::U32(128));
    if !sliding {
        return model;
    }
    let pattern: Vec<u32> = (0..30)
        .map(|layer| u32::from((layer + 1) % 6 != 0))
        .collect();
    model
        .with("gemma4.attention.sliding_window", Value::U32(64))
        .with("gemma4.attention.key_length_swa", Value::U32(64))
        .with("gemma4.attention.value_length_swa", Value::U32(64))
        .with(
            "gemma4.attention.sliding_window_pattern",
            Value::U32s(pattern),
        )
}

#[test]
fn a_sliding_window_layer_caches_its_window_not_the_whole_context() {
    // Twenty-five of the thirty layers attend to sixty-four tokens, at half
    // the key width, and five attend to the whole 1024-token context. Charging
    // every layer the full context at the full width is 30 MiB where the
    // server allocates closer to 6.
    //
    // Measured on this estate: Gemma 4 26B derives 32,040 MiB against 16,764
    // MiB it was found to hold, and the whole of that gap is this.
    let dense = estimated("catalog-unwindowed", &windowed(false));
    let sliding = estimated("catalog-windowed", &windowed(true));

    assert!(
        dense - sliding >= 23,
        "a windowed model must cost far less than the same model attending \
         fully on every layer: dense {dense} MiB, sliding {sliding} MiB"
    );
    // By hand, in bytes per layer: a full one holds 1024 tokens x 2 heads x
    // (128 + 128) x 2 bytes, which is 1 MiB, and a windowed one 64 x 2 x
    // (64 + 64) x 2, which is 1/32 of that. Five and twenty-five of them are
    // 5.78 MiB, beside 64 MiB of weights, 3.2 of fragmentation and 1024 of
    // overhead: 1096.98 MiB, rounded up.
    assert_eq!(
        sliding, 1097,
        "each layer costed at its own span, heads and widths"
    );
}

/// A model of twenty-six layers that says it slides, without saying which
/// layers do.
///
/// This is the shape Gemma 3 ships: `attention.sliding_window` and nothing
/// beside it. Which layers take the window is fixed at one full-attention
/// layer in every six, and that lives in llama.cpp's loader rather than in the
/// file, so a reader waiting for an array it will never see charges every
/// layer the whole context.
fn unpatterned(sliding: bool) -> Gguf {
    let model = Gguf::model("gemma3", 26, 8192, 256)
        .with("gemma3.attention.head_count", Value::U32(4))
        .with("gemma3.attention.head_count_kv", Value::U32(1))
        .with("gemma3.attention.key_length", Value::U32(256))
        .with("gemma3.attention.value_length", Value::U32(256));
    if sliding {
        model.with("gemma3.attention.sliding_window", Value::U32(64))
    } else {
        model
    }
}

#[test]
fn a_window_an_architecture_does_not_spell_out_is_still_a_window() {
    // Twenty-two of these twenty-six layers see sixty-four tokens and four see
    // the whole 1024-token context, but the file says only that a window
    // exists. Reading it as dense charges 26 MiB where the server allocates
    // closer to 5.
    //
    // Measured on this estate: Gemma 3 1B derives 2664 MiB against the 2048 it
    // declares, and the whole of that 616 MiB gap is this.
    let dense = estimated("catalog-unpatterned-dense", &unpatterned(false));
    let sliding = estimated("catalog-unpatterned", &unpatterned(true));

    assert!(
        dense - sliding >= 18,
        "a model that declares a window without a pattern must still be \
         costed at its window: dense {dense} MiB, sliding {sliding} MiB"
    );
    // By hand: a full layer holds 1024 tokens x 1 head x (256 + 256) x 2
    // bytes, which is 1 MiB, and a windowed one 64 tokens of the same, 1/16
    // of that. Four and twenty-two of them are 5.375 MiB, beside 64 MiB of
    // weights, 3.2 of fragmentation and 1024 of overhead: 1096.575 MiB,
    // rounded up. The four are layers 6, 12, 18 and 24, the last of each run
    // of six.
    assert_eq!(
        sliding, 1097,
        "the pattern the loader assumes, one full layer in every six"
    );
}
