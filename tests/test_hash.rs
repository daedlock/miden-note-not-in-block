// use miden_objects::assembly::Assembler;
// use miden_objects::Felt;
// use miden_objects::Hasher;
// use vm_processor::DefaultHost;
// use vm_processor::ExecutionOptions;
// use vm_processor::StackInputs;
// use vm_processor::Word;

// #[test]
// fn test_hash() {
//     let hash_inputs: &[Felt] = &[Felt::from(110 as u8), Felt::from(120 as u8)];

//     // From Rust
//     let hash = Hasher::hash_elements(hash_inputs);

//     // From VM
//     let masm = r#"
//       begin
//       debug.stack
//       hash
//       debug.stack
//       end
//     "#;

//     let assembler = Assembler::default().with_debug_mode(true);
//     let program = assembler.compile(masm).unwrap();
//     let result = vm_processor::execute(
//         &program,
//         StackInputs::new(hash_inputs.to_vec()),
//         DefaultHost::default(),
//         ExecutionOptions::default(),
//     )
//     .unwrap();

//     let stack_out = result
//         .stack_outputs()
//         .stack()
//         .iter()
//         .take(4)
//         .rev()
//         .cloned()
//         .collect::<Vec<_>>();

//     println!("Hash: {:?}", hash);
//     println!("Result: {:?}", stack_out);
// }
