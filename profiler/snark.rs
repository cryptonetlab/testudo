// #![allow(non_snake_case)]
// #![allow(clippy::assertions_on_result_states)]

// extern crate libspartan;
// extern crate merlin;

// use ark_serialize::*;
// use libspartan::parameters::poseidon_params;
// use libspartan::poseidon_transcript::PoseidonTranscript;
// use libspartan::{Instance, SNARKGens, SNARK};
// type F = ark_bls12_377::Fr;
// type E = ark_bls12_377::Bls12_377;

// fn print(msg: &str) {
//   let star = "* ";
//   println!("{:indent$}{}{}", "", star, msg, indent = 2);
// }

// pub fn main() {
//   // the list of number of variables (and constraints) in an R1CS instance
//   let inst_sizes = vec![10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];

//   println!("Profiler:: SNARK");
//   for &s in inst_sizes.iter() {
//     let num_vars = (2_usize).pow(s as u32);
//     let num_cons = num_vars;
//     let num_inputs = 10;

//     // produce a synthetic R1CSInstance
//     let (inst, vars, inputs) =
//       Instance::<F>::produce_synthetic_r1cs(num_cons, num_vars, num_inputs);

//     // produce public generators
//     let gens = SNARKGens::<E>::new(num_cons, num_vars, num_inputs, num_cons, poseidon_params());

//     // create a commitment to R1CSInstance
//     let (comm, decomm) = SNARK::encode(&inst, &gens);

//     let params = poseidon_params();

//     // produce a proof of satisfiability
//     let mut prover_transcript = PoseidonTranscript::new(&params);
//     let proof = SNARK::prove(
//       &inst,
//       &comm,
//       &decomm,
//       vars,
//       &inputs,
//       &gens,
//       &mut prover_transcript,
//     );

//     let mut proof_encoded = Vec::new();
//     proof
//       .serialize_with_mode(&mut proof_encoded, Compress::Yes)
//       .unwrap();
//     let msg_proof_len = format!("SNARK::proof_compressed_len {:?}", proof_encoded.len());
//     print(&msg_proof_len);

//     // verify the proof of satisfiability
//     let mut verifier_transcript = PoseidonTranscript::new(&params);
//     assert!(proof
//       .verify(&comm, &inputs, &mut verifier_transcript, &gens, params)
//       .is_ok());

//     println!();
//   }
// }
fn main() {}
