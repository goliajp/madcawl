// the batch's tests, one per function of `mod.rs`; the including file
// names its engine `E` and implements `batch::Driver` for it

#[test]
fn elements_are_made_moved_and_read_back_as_the_same_wrappers() {
    batch::elements_are_made_moved_and_read_back_as_the_same_wrappers::<E>();
}

#[test]
fn text_content_reads_and_replaces_children() {
    batch::text_content_reads_and_replaces_children::<E>();
}

#[test]
fn wrong_receivers_and_wrong_trees_throw_the_right_errors() {
    batch::wrong_receivers_and_wrong_trees_throw_the_right_errors::<E>();
}

#[test]
fn methods_constants_and_readonly_attributes_behave_as_members() {
    batch::methods_constants_and_readonly_attributes_behave_as_members::<E>();
}

#[test]
fn interface_objects_hold_the_prototype_chain() {
    batch::interface_objects_hold_the_prototype_chain::<E>();
}

#[test]
fn attributes_and_ids_of_a_parsed_document() {
    batch::attributes_and_ids_of_a_parsed_document::<E>();
}

#[test]
fn listeners_are_added_once_called_and_removed() {
    batch::listeners_are_added_once_called_and_removed::<E>();
}

#[test]
fn an_event_type_is_one_name_however_it_is_spelled() {
    batch::an_event_type_is_one_name_however_it_is_spelled::<E>();
}

#[test]
fn a_dom_exception_carries_name_message_and_code() {
    batch::a_dom_exception_carries_name_message_and_code::<E>();
}

#[test]
fn a_closed_host_leaves_a_sound_dom() {
    batch::a_closed_host_leaves_a_sound_dom::<E>();
}

#[test]
fn timers_and_microtasks_run_on_the_event_loop() {
    batch::timers_and_microtasks_run_on_the_event_loop::<E>();
}

#[test]
fn a_detached_tree_script_let_go_of_is_freed_before_close() {
    batch::a_detached_tree_script_let_go_of_is_freed_before_close::<E>();
}

#[test]
fn holding_one_wrapper_keeps_its_whole_tree() {
    batch::holding_one_wrapper_keeps_its_whole_tree::<E>();
}

#[test]
fn the_arena_gives_back_what_a_loop_of_detached_trees_took() {
    batch::the_arena_gives_back_what_a_loop_of_detached_trees_took::<E>();
}

#[test]
fn dispatch_runs_capture_target_and_bubble_listeners_in_order() {
    batch::dispatch_runs_capture_target_and_bubble_listeners_in_order::<E>();
}

#[test]
fn a_listener_removed_during_dispatch_is_not_called_even_when_added_again() {
    batch::a_listener_removed_during_dispatch_is_not_called_even_when_added_again::<E>();
}

#[test]
fn removing_nodes_during_dispatch_leaves_its_path_as_it_was() {
    batch::removing_nodes_during_dispatch_leaves_its_path_as_it_was::<E>();
}

#[test]
fn an_event_keeps_its_detached_target_wrapper_with_its_own_properties() {
    batch::an_event_keeps_its_detached_target_wrapper_with_its_own_properties::<E>();
}

#[test]
fn a_listener_object_holding_its_own_detached_node_is_freed() {
    batch::a_listener_object_holding_its_own_detached_node_is_freed::<E>();
}

#[test]
fn a_custom_event_whose_detail_holds_it_is_freed() {
    batch::a_custom_event_whose_detail_holds_it_is_freed::<E>();
}
