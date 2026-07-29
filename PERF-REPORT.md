# Perf: streaming card read (`perf/stream-card-read`)

## The defect

`read_card` (`src/card.rs:214`, pre-change) called `f.read_to_end(&mut buf)` on
the entire appended payload before parsing a single byte of it. Benchmarked
head-to-head against the C# tool this replaces (500 real cards, 6.08 GB, page
cache warmed): C# 1.22 s / 5088 MB/s, Rust 2.56 s / 2430 MB/s — Rust 2.1x
slower, entirely attributable to the over-read (measured separately at 31.9x
over 200 real cards: 651.7 MB slurped vs. 20.5 MB actually needed to reach the
block table, and most of even that 20.5 MB is the embedded face image, which
never needs to be read at all).

## TDD steps, both RED observations

**First RED — compile failure.** Added `parsing_a_card_does_not_read_its_whole_payload`
in `src/card.rs`, calling a not-yet-existing `read_card_from`:

```
error[E0425]: cannot find function `read_card_from` in this scope
   --> src\card.rs:332:17
    |
332 |         let m = read_card_from(&mut cr).expect("card");
    |                 ^^^^^^^^^^^^^^ not found in this scope
```

**Second RED — the test can actually catch the defect.** Added `read_card_from`
with the ORIGINAL slurping body (`read_to_end` then parse), generalised only
enough to compile against `R: Read + Seek`. Running the test:

```
thread 'card::tests::parsing_a_card_does_not_read_its_whole_payload' panicked at src\card.rs:345:9:
expected under 256 KiB actually read, got 16777397 bytes
test card::tests::parsing_a_card_does_not_read_its_whole_payload ... FAILED
```

`16777397 = 16*1024*1024 (16 MiB filler) + 181` (the real card's own bytes) —
confirms the test is reading through the entire tail, exactly the defect being
guarded against.

**GREEN.** Converted the parse to stream from the reader (details below).
Re-running the same test:

```
TEMP bytes_read=181
test card::tests::parsing_a_card_does_not_read_its_whole_payload ... ok
```

(the `TEMP` print was a scratch instrumentation line, removed before commit —
181 bytes is the number the committed test asserts is `< 256 * 1024`).

**Before → after: 16,777,397 bytes → 181 bytes** for a fixture with a 16 MiB
tail after the `Parameter` block.

## What changed

- `src/png.rs`: `payload_span` generalised from `&mut File` to
  `<R: Read + Seek>(r: &mut R)`. It can no longer call `File::metadata()` for
  the length, so it now gets it the way a lazy `BinaryReader` would — seek to
  `SeekFrom::End(0)`, record the position, seek back to `0`. `read_or_eof` was
  generalised the same way. No behavioural change; all of `png.rs`'s own tests
  (which call it on a real `File`) pass unchanged.
- `src/card.rs`:
  - `read_card(path)` is now a thin wrapper: open the file, delegate to
    `read_card_from`.
  - New `pub fn read_card_from<R: Read + Seek>(r: &mut R) -> Result<CardMeta, CardError>`
    holds the actual parse.
  - The old `Cur<'a>` (a cursor over an in-memory `&'a [u8]`) is replaced by
    `StreamCur<'a, R>`, a cursor over the reader itself. Its `take`/`skip` both
    check the requested length against a `remaining` counter — seeded from the
    payload length `payload_span` measured off the real file — **before**
    touching the reader, so a hostile or corrupt `faceLen`/table-length/
    `pos`/`size` can never allocate or seek past what the file actually
    contains. `take` reads and returns bytes (used for the small fixed-size
    fields and the block table, which do need to be examined); `skip` seeks
    past bytes without reading them (used for the embedded face image, which
    never needs to be examined).
  - The face image is skipped with `StreamCur::skip` instead of read — this is
    the single biggest win, since a real card's face image dwarfs everything
    else in the appended payload.
  - Once the block table is decoded and the `Parameter` entry's `pos`/`size`
    are known, the code seeks directly to that absolute offset
    (`blocks_at + pos`, bounds-checked against the real payload length first)
    and reads exactly `size` bytes — matching the C# tool's lazy
    `BinaryReader` seeking straight to the `Parameter` block.
  - A small `read_exact_mapped` helper distinguishes a genuine I/O failure
    (`ErrorKind::UnexpectedEof` aside) from the stream simply ending early,
    which is `CardError::Malformed` — the streaming equivalent of the old
    `buf.get(start..end)` returning `None`.
- `src/counting_reader.rs` (new, test-only): `CountingReader<R>` wraps any
  `Read + Seek`, forwards `Read::read` while accumulating a byte counter, and
  forwards `Seek::seek` untouched — a seek costs nothing against the counter,
  which is the whole point of the test it supports. Declared
  `#[cfg(test)] mod counting_reader;` in `main.rs`, following the same pattern
  as `fixture.rs` and `tempdir.rs`.
- `src/card.rs` test module: new test
  `parsing_a_card_does_not_read_its_whole_payload`, plus imports for
  `CountingReader` and `std::io::Cursor`.

No public error taxonomy, error-to-condition mapping, or observable behaviour
changed. All existing assertions in `card.rs` and `main.rs` (I/O vs. NotCard vs.
Malformed vs. Unrecognized vs. Scene, `{Game}/Unknown` filing, truncation
handling, etc.) pass unchanged.

## Full `cargo test` output

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.06s
     Running unittests src\main.rs (target\debug\deps\koikatsu_hamster-9b7b48de2dfbabcc.exe)

running 78 tests
test card::tests::every_game_folder_is_listed_in_dest_folders_and_vice_versa ... ok
test msgpack::tests::nesting_within_the_cap_still_decodes ... ok
test msgpack::tests::a_non_utf8_string_decodes_lossily_instead_of_failing ... ok
test msgpack::tests::decodes_a_string_keyed_map_like_a_card_parameter_block ... ok
test msgpack::tests::decodes_nested_array_of_maps_like_a_block_table ... ok
test msgpack::tests::truncated_input_is_an_error_not_a_panic ... ok
test msgpack::tests::wide_types_decode ... ok
test plan::tests::a_card_already_sitting_in_its_destination_is_recognised_as_filed ... ok
test plan::tests::a_card_pack_folder_named_like_a_koikatsu_export_is_not_excluded ... ok
test msgpack::tests::pathological_nesting_is_an_error_not_a_stack_overflow ... ok
test plan::tests::a_fixed_route_ignores_the_search_term_because_it_has_no_name ... ok
test plan::tests::a_fixed_route_uses_its_own_leaf ... ok
test plan::tests::a_folder_named_unknown_something_is_not_excluded ... ok
test plan::tests::a_female_character_card_goes_to_game_female ... ok
test plan::tests::a_game_named_folder_below_the_first_level_is_not_an_output_folder ... ok
test plan::tests::a_matching_search_term_adds_a_subfolder_and_a_non_match_does_not ... ok
test plan::tests::a_path_outside_the_root_is_not_excluded ... ok
test plan::tests::an_unknown_sex_still_gets_a_folder_rather_than_being_dropped ... ok
test plan::tests::output_folders_directly_under_the_root_are_excluded ... ok
test card::tests::a_coordinate_card_routes_to_a_fixed_folder_and_needs_no_parameter_block ... ok
test card::tests::a_card_locked_by_another_process_is_an_io_error_not_a_non_card ... ok
test card::tests::a_non_png_is_not_a_card ... ok
test card::tests::a_card_with_no_parameter_entry_is_filed_under_unknown_rather_than_failed ... ok
test card::tests::an_emotion_creators_card_gets_its_own_folder ... ok
test card::tests::a_parameter_block_running_past_the_end_is_malformed_not_a_panic ... ok
test card::tests::sex_zero_is_male ... ok
test card::tests::a_path_that_cannot_be_read_is_an_io_error_not_a_non_card ... ok
test card::tests::an_undecodable_parameter_block_is_filed_under_unknown_rather_than_failed ... ok
test plan::tests::a_free_name_is_returned_unchanged_when_nothing_is_there ... ok
test card::tests::a_structurally_broken_card_is_still_an_error_not_filed_under_unknown ... ok
test card::tests::reads_a_koikatu_female_character_card ... ok
test card::tests::a_plain_image_is_not_a_card ... ok
test card::tests::an_unknown_marker_is_reported_verbatim_never_guessed ... ok
test card::tests::a_scene_card_is_recognized_by_its_version_string ... ok
test card::tests::a_missing_sex_field_yields_unknown_rather_than_an_error ... ok
test card::tests::the_product_no_prefix_and_version_0_0_0_are_handled ... ok
test tests::a_root_value_that_looks_like_a_flag_is_rejected_rather_than_swallowed ... ok
test plan::tests::a_non_utf8_file_name_keeps_its_exact_bytes_through_the_collision_path ... ok
test tests::a_search_term_that_could_relocate_cards_is_rejected_at_parse_time ... ok
test card::tests::a_recognized_marker_is_never_reclassified_as_a_scene ... ok
test card::tests::a_non_integer_sex_field_yields_unknown_rather_than_an_error ... ok
test tests::an_unknown_flag_is_an_error ... ok
test tests::an_unresolvable_working_directory_is_a_reported_error_not_a_panic ... ok
test tests::flags_and_a_positional_search_term_parse ... ok
test tests::no_arguments_means_current_directory_and_a_real_move ... ok
test tests::root_without_a_value_is_an_error_rather_than_a_silent_default ... ok
test tests::the_summary_prints_every_count_it_keeps ... ok
test png::tests::a_chunk_length_running_past_eof_is_none ... ok
test plan::tests::a_name_without_an_extension_still_gets_a_counter ... ok
test plan::tests::a_taken_name_gains_a_counter ... ok
test png::tests::a_non_png_is_none ... ok
test png::tests::an_iend_signature_inside_image_data_is_not_mistaken_for_the_real_one ... ok
test png::tests::a_plain_image_has_a_zero_length_payload ... ok
test png::tests::finds_the_payload_after_iend ... ok
test tests::a_nonexistent_root_is_reported_rather_than_silently_producing_an_empty_run ... ok
test tests::a_card_already_in_its_destination_is_counted_and_not_moved ... ok
test walk::tests::an_empty_root_yields_nothing ... ok
test tests::a_card_with_a_non_utf8_name_is_moved_under_that_exact_name ... ok
test tests::pruning_empty_directories_stops_at_the_root_and_at_anything_not_empty ... ok
test tests::a_card_that_cannot_be_read_is_reported_as_an_error_not_counted_as_a_texture ... ok
test tests::a_malformed_card_is_counted_as_an_error_and_left_alone ... ok
test tests::a_dry_run_reports_the_same_thing_and_moves_nothing ... ok
test tests::a_dry_run_previews_the_free_name_against_files_already_on_disk ... ok
test tests::a_character_card_whose_sex_cannot_be_read_is_filed_under_unknown_not_failed ... ok
test walk::tests::a_relative_root_still_skips_its_own_output_folders ... ok
test tests::a_double_click_inside_an_organised_game_folder_never_renames_a_card_onto_itself ... ok
test tests::a_second_run_moves_nothing_because_the_output_folder_is_skipped ... ok
test tests::a_name_collision_at_the_destination_gains_a_counter ... ok
test walk::tests::returns_the_full_result_sorted_not_just_sorted_within_each_directory ... ok
test walk::tests::skips_this_programs_own_output_folders ... ok
test walk::tests::finds_pngs_at_any_depth_and_ignores_other_extensions ... ok
test walk::tests::does_not_skip_card_pack_folders_that_merely_contain_a_game_name ... ok
test tests::a_search_term_sorts_matches_into_a_subfolder_without_filtering_the_rest ... ok
test tests::a_run_files_each_card_and_counts_everything_else ... ok
test walk::tests::a_directory_junction_pointing_at_an_ancestor_does_not_loop_forever ... ok
test walk::tests::a_symlinked_png_is_skipped_but_counted ... ok
test card::tests::parsing_a_card_does_not_read_its_whole_payload ... ok
test png::tests::iend_straddling_a_4096_byte_boundary_is_still_found ... ok

test result: ok. 78 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s

     Running tests\cli.rs (target\debug\deps\cli-bd85ff7220df2780.exe)

running 5 tests
test a_search_term_containing_a_path_separator_exits_two_with_usage ... ok
test an_unknown_flag_exits_two_with_usage ... ok
test a_nonexistent_root_is_reported_on_stderr_and_exits_nonzero ... ok
test a_malformed_card_exits_one_without_waiting_for_input ... ok
test organises_a_tree_prints_a_banner_and_exits_zero ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

78 + 5 = 83 tests total: the 82 pre-existing tests, unchanged, plus the one new
regression test. All green.

## Full `cargo clippy --all-targets -- -D warnings` output

```
    Checking koikatsu-hamster v0.1.0 (C:\Users\weiss\Desktop\ws\koikatsu-hamster-rs)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.31s
```

Clean — no warnings, `[dependencies]` and `[dev-dependencies]` still empty.

## Decisions this brief didn't settle

- **Where `CountingReader` lives**: a new `src/counting_reader.rs`, gated
  `#[cfg(test)]` in `main.rs`, mirroring the existing `fixture.rs`/`tempdir.rs`
  one-utility-per-file convention rather than nesting it inside `card.rs`'s own
  test module.
- **How `payload_span` gets the stream length without `File::metadata()`**:
  seek to `SeekFrom::End(0)` and back to `0`, rather than requiring the caller
  to pass a length in. Two extra seeks (cheap, not reads) per card; unchanged
  for the real `File` path since `File::seek` is just as cheap as
  `File::metadata()` for this purpose.
- **How the absolute offset of the block region (`blocks_at`) is derived**:
  computed as `off + (len - remaining)` from the byte-accounting `StreamCur`
  already keeps, rather than calling `Seek::stream_position()`. Same value,
  one fewer syscall.
- **Bounds-check style for `pos`/`size`**: kept the original's `saturating_add`
  (now on `u64`) rather than switching to `checked_add`-with-explicit-overflow-
  error. A saturated `end` still fails the `end > payload_end` check, so a
  hostile huge `pos`/`size` is still rejected as `Malformed`, and the code
  reads slightly closer to the pre-change version it replaces.
- **Error message text on the rare EOF-during-final-read race**: if the
  underlying file shrinks between the `end > payload_end` bounds check and the
  seek+read of the `Parameter` block itself (a real race only against a file
  changing concurrently, not reachable from any test here), the error text is
  `"truncated card"` rather than `"Parameter block runs past end of card"`.
  Both are `CardError::Malformed`, so no test or caller distinguishes them —
  flagging in case the exact wording ever matters elsewhere.

## Not done, per instructions — flagged instead

`walk::candidates` and the move loop in `main.rs` were explicitly out of scope
for this branch. One thing noticed in passing: `plan::free_name` calls
`Path::exists()` once per candidate name when resolving a collision, which is
an extra `stat` per collision but is `O(collisions)`, not `O(payload size)` —
nowhere near the magnitude of the read-path defect this branch fixes. Not
touched.
