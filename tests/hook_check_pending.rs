// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The tests whose programs still give a value with a drop hook a second
//! holder for a moment, so the harness runs them without `MIRI_HOOK_CHECK`.
//!
//! Every other program the suite runs is checked: a resource has one owner,
//! and the runtime stops a program whose hooked block gains a second holder.
//! Each entry names the mechanism that still leaves one; the entry goes when
//! the mechanism is fixed, and none is added for a new test.

/// An `out` place inside a class instance is copied in with a reference of
/// its own and stored back after the call, since a class instance is still
/// shared on assignment and another binding may reach the slot while the call
/// runs. `take()` and `replace` reach a place this way.
const OUT_SLOT: &str = "an out place inside a class instance is copied in and stored back";

/// A generic body, or the adapter a runtime closure slot calls, lends what
/// it is handed at a resource type and keeps a funded copy where it stores,
/// returns or hands it on.
const GENERIC_BODY: &str =
    "a generic body or closure adapter keeps a funded copy at a resource type";

/// A resource read out of a plain field, an array element, a tuple or a map
/// value is handed to a keeper as a funded copy while the place still holds
/// it. These programs are to be refused, with a repair that moves the value
/// out of its place.
const FUNDED_PLACE: &str = "a resource read out of a place is handed to a keeper as a funded copy";

/// A map or set getter hands out an owned copy of the element it reads, a
/// reference of its own beside the collection's.
const OWNED_GETTER: &str = "a map or set getter hands out an owned copy";

/// A field of a class instance lent to a call that may reach the instance
/// another way is copied with a reference of its own for the call, since a
/// class instance is still shared on assignment and another holder may
/// replace the field while the call runs.
const CLASS_FIELD_ARGUMENT: &str = "a class field lent to a call is copied for it";

/// A payload of an enum held as a counted block is copied out of it for a
/// keeper, then the enum is released whole: moving one field out needs a
/// release that frees the block without the fields already moved.
const ENUM_PAYLOAD: &str = "an enum block's payload handed to a keeper is copied out of it";

/// A keeper called on only some paths of a match arm is handed a funded copy
/// of the payload, which stays bound on the other paths.
const CONDITIONAL_HANDOVER: &str = "a payload handed over on some paths only is a funded copy";

/// A closure lent to a call holds a reference of its own to a resource it
/// only reads, beside the binding that owns it.
const CAPTURED_RESOURCE: &str = "a lent closure takes a reference of its own to what it reads";

/// Each test, by its path from the test binary's root, with the mechanism
/// that still gives its program a second holder.
pub(crate) const PENDING: &[(&str, &str)] = &[
    ("integration::memory::generic_places_handed_to_keepers::replace_runs_at_a_resource_instantiation", OUT_SLOT),
    ("integration::memory::kept_receivers::an_out_parameter_replaced_for_a_kept_receiver_hands_over_its_old_value", OUT_SLOT),
    ("integration::memory::option_take::a_sibling_operand_sees_an_optional_resource_before_take_empties_it", OUT_SLOT),
    ("integration::memory::option_take::take_on_a_field_leaves_none_in_the_field", OUT_SLOT),
    ("integration::memory::option_take::take_on_a_list_element_inside_a_loop_over_indices", OUT_SLOT),
    ("integration::memory::option_take::take_on_a_list_element_leaves_none_in_the_list", OUT_SLOT),
    ("integration::memory::out_places::a_plain_resource_field_is_replaced_through_a_var", OUT_SLOT),
    ("integration::memory::out_places_refused::a_field_of_an_element_copied_in_beside_its_list_is_stored_back", OUT_SLOT),
    ("integration::memory::out_places_refused::a_field_of_the_receiver_copied_in_for_its_own_method_is_stored_back", OUT_SLOT),
    ("integration::memory::out_places_refused::a_kept_parameter_passed_explicitly_beside_a_resource_taken_out_runs", OUT_SLOT),
    ("integration::memory::out_places_refused::a_resource_field_copied_in_beside_the_resource_holding_it_is_stored_back", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::replacing_a_generic_out_parameter_runs_at_a_resource_instantiation", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::replacing_a_restored_receiver_hands_its_old_value_back", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::replacing_an_out_parameter_hands_its_old_value_to_the_keeper", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::taking_an_optional_field_of_an_element_hands_it_over", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::taking_an_optional_field_through_a_function_value_hands_it_over", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::taking_an_optional_field_under_a_check_hands_it_over", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::the_suggested_take_compiles_and_runs", OUT_SLOT),
    ("integration::memory::replace::a_discarded_old_value_is_released_at_once", OUT_SLOT),
    ("integration::memory::replace::replace_runs_the_old_values_hook_where_it_ends", OUT_SLOT),
    ("integration::memory::replace::replace_writes_through_an_out_parameter", OUT_SLOT),
    ("integration::memory::replaced_receivers::a_receiver_restored_by_its_method_is_not_consumed_by_the_call", OUT_SLOT),
    ("integration::memory::replaced_receivers::a_resource_method_taking_its_old_self_leaves_the_caller_owning_both", OUT_SLOT),
    ("integration::generics::field_arguments::a_generic_method_building_its_swapped_instance_releases_everything", GENERIC_BODY),
    ("integration::generics::generic_methods::a_generic_method_passing_a_resource_drops_it_once", GENERIC_BODY),
    ("integration::generics::loop_variables::a_generic_loop_releases_each_class_element_it_wraps", GENERIC_BODY),
    ("integration::memory::closures::a_generic_lambda_at_a_scalar_and_at_a_resource_releases_each_correctly", GENERIC_BODY),
    ("integration::memory::collection_element_copies::a_list_of_a_resource_that_is_never_copied_compiles", GENERIC_BODY),
    ("integration::memory::collection_element_copies::copying_lists_built_in_generic_bodies_clones_each_element", GENERIC_BODY),
    ("integration::memory::collection_element_copies::filtering_a_list_of_a_cloneable_resource_clones_each_element_kept", GENERIC_BODY),
    ("integration::memory::collection_element_copies::mapping_a_list_of_a_cloneable_resource_copies_nothing", GENERIC_BODY),
    ("integration::memory::collection_element_copies::mapping_a_list_of_a_resource_that_is_not_cloneable_copies_nothing", GENERIC_BODY),
    ("integration::memory::conditional_branches::test_conditional_reading_resource_local_runs_hook_once", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_generic_binding_returned_from_where_it_moved_runs_at_a_resource_type", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_generic_function_keeping_a_resource_lends_and_the_caller_releases_after_the_call", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_generic_keeper_at_a_resource_type_hands_it_to_the_result", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_local_rebound_away_from_a_parameter_no_longer_keeps_it", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_loop_binding_shadowing_a_parameter_hides_it", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_resource_an_if_arm_tail_returns_is_released_on_each_path", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_base_value_built_in_a_generic_function_at_a_resource_is_released_by_the_keeper", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_resource_instance_only_an_unread_slot_would_build_leaves_the_trait_plain", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_call_in_a_generic_function_called_at_a_plain_type_leaves_the_trait_plain", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_call_in_a_generic_function_nothing_calls_leaves_the_trait_plain", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_call_through_a_field_reads_the_slot_at_the_fields_type", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_value_built_around_a_parameter_at_a_resource_is_released_by_the_keeper", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_value_built_around_a_type_nested_to_the_depth_bound_is_released_by_the_keeper", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_value_built_in_a_generic_function_at_a_resource_is_released_by_the_keeper", GENERIC_BODY),
    ("integration::memory::generic_function_instantiations::a_trait_value_built_two_generic_calls_deep_at_a_resource_is_released_by_the_keeper", GENERIC_BODY),
    ("integration::memory::kept_default_and_generic_methods::a_method_storing_a_type_parameter_once_moves_it_at_a_copy_built_in_a_generic_function", GENERIC_BODY),
    ("integration::memory::kept_enum_and_super_receivers::a_parameter_handed_to_a_reader_held_in_a_local_stays_the_callers", GENERIC_BODY),
    ("integration::memory::kept_function_values::a_function_value_in_an_instantiated_generic_body_funds_its_lent_argument", GENERIC_BODY),
    ("integration::memory::kept_function_values::a_lambda_in_an_instantiated_generic_body_hands_its_value_on_once", GENERIC_BODY),
    ("integration::memory::kept_function_values::an_async_function_named_as_a_value_releases_its_argument_once_it_returns", GENERIC_BODY),
    ("integration::memory::lent_function_parameters::a_call_through_a_lending_parameter_lends_an_element_of_the_receiver", GENERIC_BODY),
    ("integration::memory::lent_function_parameters::a_function_reading_what_every_class_lends_runs", GENERIC_BODY),
    ("integration::memory::lent_function_parameters::a_library_mapping_building_new_resources_runs_every_hook_once", GENERIC_BODY),
    ("integration::memory::lent_function_parameters::a_library_predicate_reads_each_resource_element_it_is_lent", GENERIC_BODY),
    ("integration::memory::lent_function_parameters::a_named_function_that_only_reads_is_passed_to_a_lending_parameter", GENERIC_BODY),
    ("integration::memory::lent_function_parameters::a_reducer_returning_its_accumulator_runs_at_a_resource", GENERIC_BODY),
    ("integration::memory::owned_parameters::test_a_list_of_values_with_a_drop_hook_is_dropped_where_it_was", GENERIC_BODY),
    ("integration::memory::owned_parameters::test_a_parameter_with_a_drop_hook_is_dropped_where_it_was", GENERIC_BODY),
    ("integration::memory::parameter_writes::test_a_struct_parameter_holding_a_resource_runs_its_drop_hook_once", GENERIC_BODY),
    ("integration::memory::runtime_closure_slots::a_closure_read_from_a_field_is_adapted_and_released_once", GENERIC_BODY),
    ("integration::memory::runtime_closure_slots::a_runtime_sort_named_as_a_value_runs_each_hook_once", GENERIC_BODY),
    ("integration::memory::runtime_closure_slots::an_in_place_sort_over_resources_runs_each_hook_once_where_the_list_ends", GENERIC_BODY),
    ("integration::memory::runtime_closure_slots::sorted_by_over_resources_runs_each_hook_once_where_the_lists_end", GENERIC_BODY),
    ("integration::memory::runtime_closure_slots::sorted_by_with_a_named_comparator_runs_each_hook_once", GENERIC_BODY),
    ("integration::option::nullable_pointer_aliases::test_an_alias_naming_an_optional_class_is_that_optional", GENERIC_BODY),
    ("integration::memory::consumed_arguments::a_field_handed_to_a_keeper_is_released_with_the_object_holding_it", FUNDED_PLACE),
    ("integration::memory::emptying_methods::an_element_of_a_collection_taking_nothing_out_is_still_a_funded_copy", FUNDED_PLACE),
    ("integration::memory::kept_default_and_generic_methods::a_field_handed_to_a_keeping_default_stays_its_holders", FUNDED_PLACE),
    ("integration::memory::kept_function_values::a_field_handed_to_a_keeper_value_runs_its_hook_once_where_the_holder_ends", FUNDED_PLACE),
    ("integration::memory::kept_receivers::a_field_receiver_is_funded_and_its_holder_still_releases_it", FUNDED_PLACE),
    ("integration::memory::kept_receivers::a_writing_keeper_called_on_a_field_leaves_the_field_readable", FUNDED_PLACE),
    ("integration::memory::kept_virtual_calls::a_field_handed_to_a_keeper_through_its_slot_stays_its_holders", FUNDED_PLACE),
    ("integration::memory::kept_virtual_calls::a_field_handed_to_an_owning_slot_is_funded_and_its_holder_releases_it", FUNDED_PLACE),
    ("integration::memory::places_handed_to_keepers::a_map_value_handed_to_a_keeper_is_still_a_funded_copy", FUNDED_PLACE),
    ("integration::memory::places_handed_to_keepers::a_plain_resource_field_handed_to_a_keeper_is_still_a_funded_copy", FUNDED_PLACE),
    ("integration::memory::places_handed_to_keepers::an_array_element_handed_to_a_keeper_is_still_a_funded_copy", FUNDED_PLACE),
    ("integration::memory::places_handed_to_keepers::an_array_loop_binding_handed_to_a_keeper_is_still_a_funded_copy", FUNDED_PLACE),
    ("integration::memory::places_handed_to_keepers::an_element_stored_in_a_tuple_and_handed_to_a_keeper_is_still_a_funded_copy", FUNDED_PLACE),
    ("integration::map::functional::reduce_over_a_resource_class_named_like_the_key_parameter", OWNED_GETTER),
    ("integration::memory::collection_element_copies::a_map_filter_keeping_nothing_clones_nothing", OWNED_GETTER),
    ("integration::memory::collection_element_copies::cloning_a_set_of_maps_of_a_cloneable_resource_clones_each_inner_value", OWNED_GETTER),
    ("integration::memory::collection_element_copies::filtering_a_map_of_a_cloneable_resource_clones_each_value_kept", OWNED_GETTER),
    ("integration::memory::collection_element_copies::filtering_a_set_of_a_cloneable_resource_clones_each_element_kept", OWNED_GETTER),
    ("integration::memory::kept_imported_calls::library_bodies_reusing_a_resource_run_each_hook_once", OWNED_GETTER),
    ("integration::memory::places_handed_to_keepers::taking_an_optional_element_before_matching_hands_its_payload_over", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::taking_an_optional_field_hands_its_payload_to_the_keeper", OUT_SLOT),
    ("integration::memory::places_handed_to_keepers::two_payloads_of_one_owned_scrutinee_handed_to_one_keeper_end_it_once", ENUM_PAYLOAD),
    ("integration::option::comparison_in_loops::test_option_compare_none_in_while_with_if_let_walk", FUNDED_PLACE),
    ("integration::option::comparison_in_loops::test_option_compare_none_in_while_with_match_walk", FUNDED_PLACE),
    ("integration::option::comparison_in_loops::test_option_compare_some_in_while_releases_payloads", FUNDED_PLACE),
    ("integration::memory::kept_operator_and_loop_calls::a_next_keeping_its_iterator_is_handed_a_reference_of_its_own_each_pass", FUNDED_PLACE),
    ("integration::memory::kept_operator_and_loop_calls::a_value_looked_for_with_in_is_still_the_callers", GENERIC_BODY),
    ("integration::memory::kept_operator_and_loop_calls::an_element_at_keeping_its_receiver_is_handed_a_reference_each_pass", FUNDED_PLACE),
    ("integration::generics::field_arguments::a_generic_instance_built_from_another_ones_optional_fields_releases_everything", GENERIC_BODY),
    ("integration::memory::collection_element_copies::moving_the_elements_out_repairs_a_refused_copy", CONDITIONAL_HANDOVER),
    ("integration::memory::consumed_arguments::a_resource_read_by_a_closure_passed_to_a_call_stays_usable", CAPTURED_RESOURCE),
    ("integration::option::nullable_pointer_aliases::test_an_optional_of_a_generic_alias_flows_through_fields_lists_and_parameters", CLASS_FIELD_ARGUMENT),
];

/// Whether the test running on this thread is checked for a second holder.
pub(crate) fn hook_check_applies() -> bool {
    let current = std::thread::current();
    let name = current.name().unwrap_or_default();
    !PENDING.iter().any(|(test, _)| *test == name)
}
