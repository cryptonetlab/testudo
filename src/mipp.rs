use super::macros::*;
use ark_ec::msm::VariableBaseMSM;
use ark_ec::ProjectiveCurve;
use ark_ec::{AffineCurve, PairingEngine};
use ark_ff::{Field, PrimeField};
use ark_poly::{DenseMultilinearExtension, MultilinearExtension};
use ark_poly_commit::multilinear_pc::data_structures::{
  Commitment_G2, CommitterKey, Proof, Proof_G1, VerifierKey,
};
use ark_poly_commit::multilinear_pc::MultilinearPC;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Read, SerializationError, Write};
use ark_std::cfg_iter;
use ark_std::One;
use ark_std::Zero;
use rayon::iter::ParallelIterator;
use rayon::prelude::IntoParallelIterator;
use rayon::prelude::*;
use std::ops::{Add, Mul, MulAssign, SubAssign};
use thiserror::Error;

#[derive(Debug, Clone, CanonicalDeserialize, CanonicalSerialize)]
pub struct MippProof<E: PairingEngine> {
  pub comms_t: Vec<(<E as PairingEngine>::Fqk, <E as PairingEngine>::Fqk)>,
  pub comms_u: Vec<(E::G1Affine, E::G1Affine)>,
  pub final_a: E::G1Affine,
  pub final_h: E::G2Affine,
  pub pst_proof_h: Proof_G1<E>,
}

impl<E: PairingEngine> MippProof<E> {
  pub fn prove<T: Transcript>(
    transcript: &mut impl Transcript,
    ck: &CommitterKey<E>,
    a: Vec<E::G1Affine>,
    y: Vec<E::Fr>,
    h: Vec<E::G2Affine>,
    U: &E::G1Affine,
    T: &<E as PairingEngine>::Fqk,
  ) -> Result<MippProof<E>, Error> {
    // the values of vectors C and bits rescaled at each step of the loop
    // these are A and y
    let (mut m_a, mut m_y) = (a.clone(), y.clone());
    // the values of the commitment keys rescaled at each step of the loop
    // these are the h for me
    let mut m_h = h.clone();

    // storing the values for including in the proof
    // these are T_l and T_r
    let mut comms_t = Vec::new();
    // these are U_l and U_r
    let mut comms_u = Vec::new();
    // these are the x-es
    let mut xs: Vec<E::Fr> = Vec::new();
    let mut xs_inv: Vec<E::Fr> = Vec::new();

    // we already appended t
    transcript.append(b"U", U);
    while m_a.len() > 1 {
      // recursive step
      // Recurse with problem of half size
      let split = m_a.len() / 2;

      // MIPP ///
      // c[:n']   c[n':]
      let (a_l, a_r) = m_a.split_at_mut(split);
      // r[:n']   r[:n']
      let (y_l, y_r) = m_y.split_at_mut(split);

      let (h_l, h_r) = m_h.split_at_mut(split);

      // since we do this in parallel we take reference first so it can be
      // moved within the macro's rayon scope.
      let (rh_l, rh_r) = (&h_l, &h_r);
      let (ra_l, ra_r) = (&a_l, &a_r);
      let (ry_l, ry_r) = (&y_l, &y_r);
      // See section 3.3 for paper version with equivalent names
      try_par! {
          // MIPP part
          // Compute cross commitment C^r
          // z_l = c[n':] ^ r[:n']
          // TODO to replace by bitsf_multiexp
          let comm_u_l = multiexponentiation(ra_l, &ry_r),
          // Z_r = c[:n'] ^ r[n':]
          let comm_u_r = multiexponentiation(ra_r, &ry_l)

      };
      // Compute C commitment over the distinct halfs of C
      // u_l = c[n':] * v[:n']
      let comm_t_l = pairings_product::<E>(&ra_l, rh_r);
      // u_r = c[:n'] * v[n':]
      let comm_t_r = pairings_product::<E>(&ra_r, rh_l);

      // Fiat-Shamir challenge
      transcript.append(b"comm_u_l", &comm_u_l);
      transcript.append(b"comm_u_r", &comm_u_r);
      transcript.append(b"comm_t_l", &comm_t_l);
      transcript.append(b"comm_t_r", &comm_t_r);
      let c_inv = transcript.challenge_scalar::<E::Fr>(b"challenge_i");

      // Optimization for multiexponentiation to rescale G2 elements with
      // 128-bit challenge Swap 'c' and 'c_inv' since can't control bit size
      // of c_inv
      let c = c_inv.inverse().unwrap();

      // Set up values for next step of recursion
      // c[:n'] + c[n':]^x
      compress(&mut m_a, split, &c);
      compress_field(&mut m_y, split, &c_inv);

      // v_left + v_right^x^-1
      compress(&mut m_h, split, &c_inv);

      comms_t.push((comm_t_l, comm_t_r));
      comms_u.push((comm_u_l.into_affine(), comm_u_r.into_affine()));
      xs.push(c);
      xs_inv.push(c_inv);
    }

    assert!(m_a.len() == 1 && m_y.len() == 1 && m_h.len() == 1);

    let final_a = m_a[0];
    let final_h = m_h[0];

    // println!("before evaluations");
    // get polynomial
    let poly = DenseMultilinearExtension::<E::Fr>::from_evaluations_vec(
      xs_inv.len(),
      Self::polynomial_evaluations_from_transcript::<E::Fr>(&xs_inv),
    );
    let c = MultilinearPC::<E>::commit_g2(ck, &poly);
    assert!(c.h_product == final_h);

    // create proof that h is indeed correct
    let mut point: Vec<E::Fr> = (0..poly.num_vars)
      .into_iter()
      .map(|_| transcript.challenge_scalar::<E::Fr>(b"random_point"))
      .collect();

    let pst_proof_h = MultilinearPC::<E>::open_g1(ck, &poly, &point);

    println!("PROVER: last challenge {}", xs.last().unwrap());
    println!("PROVER: last y {}", m_y.last().unwrap());
    println!("PROVER: last final c {:?}", m_a.last().unwrap());

    Ok(
      (MippProof {
        comms_t,
        comms_u,
        final_a,
        final_h: final_h,
        pst_proof_h,
      }),
    )
  }

  fn polynomial_evaluations_from_transcript<F: Field>(cs_inv: &[F]) -> Vec<F> {
    let m = cs_inv.len();
    let pow_m = 2_usize.pow(m as u32);

    let evals = (0..pow_m)
      .into_par_iter()
      .map(|i| {
        let mut res = F::one();
        for j in 0..m {
          if (i >> j) & 1 == 1 {
            res *= cs_inv[m - j - 1];
          }
        }
        res
      })
      .collect();
    evals
  }

  pub fn verify<T: Transcript>(
    vk: &VerifierKey<E>,
    transcript: &mut impl Transcript,
    proof: &MippProof<E>,
    point: Vec<E::Fr>,
    U: &E::G1Affine,
    T: &<E as PairingEngine>::Fqk,
  ) -> bool {
    let comms_u = proof.comms_u.clone();
    let comms_t = proof.comms_t.clone();

    let mut xs = Vec::new();
    let mut xs_inv = Vec::new();
    let mut final_y = E::Fr::one();
    let mut u_prime = U.clone().into_projective();
    let mut t_prime = T.clone();

    transcript.append(b"U", U);
    assert!(comms_u.len() == point.len());
    for (i, (comm_u, comm_t)) in comms_u.iter().zip(comms_t.iter()).enumerate() {
      let (comm_u_l, comm_u_r) = comm_u;
      let (comm_t_l, comm_t_r) = comm_t;

      // Fiat-Shamir challenge
      transcript.append(b"comm_u_l", comm_u_l);
      transcript.append(b"comm_u_r", comm_u_r);
      transcript.append(b"comm_t_l", comm_t_l);
      transcript.append(b"comm_t_r", comm_t_r);
      let c_inv = transcript.challenge_scalar::<E::Fr>(b"challenge_i");

      let c = c_inv.inverse().unwrap();
      xs.push(c);
      xs_inv.push(c_inv);
    }
    let len = point.len();

    let final_y: E::Fr = (0..len)
      .into_par_iter()
      .map(|i| E::Fr::one() + xs_inv[i].mul(point[i]) - point[i])
      .product();

    u_prime += (0..len)
      .into_iter()
      .map(|i| {
        let (comm_u_l, comm_u_r) = comms_u[i];
        comm_u_l.into_projective().mul(xs_inv[i].into_repr())
          + comm_u_r.into_projective().mul(xs[i].into_repr())
      })
      .sum::<E::G1Projective>();

    t_prime *= (0..len)
      .into_par_iter()
      .map(|i| {
        let (comm_t_l, comm_t_r) = comms_t[i];
        comm_t_l.pow(xs_inv[i].into_repr()) * comm_t_r.pow(xs[i].into_repr())
      })
      .product::<E::Fqk>();

    println!("VERIFIER: last challenge {}", xs.last().unwrap());
    println!("VERIFIER: last y {}", final_y);
    println!("VERIFIER: last final c from prover {:?}", proof.final_a);

    // compute structured polynomial h at a random point
    let mut point: Vec<E::Fr> = Vec::new();
    let m = xs_inv.len();
    for i in 0..m {
      let r = transcript.challenge_scalar::<E::Fr>(b"random_point");
      point.push(r);
    }
    let v = (0..m)
      .into_par_iter()
      .map(|i| E::Fr::one() + point[i].mul(xs_inv[m - i - 1]) - point[i])
      .product();

    // println!("VERIFIER: v is {}", v);

    let comm_h = Commitment_G2 {
      nv: m,
      h_product: proof.final_h,
    };
    let check_h = MultilinearPC::<E>::check_2(vk, &comm_h, &point, v, &proof.pst_proof_h);
    assert!(check_h == true);

    let final_u = proof.final_a.mul(final_y);
    let final_t: <E as PairingEngine>::Fqk = E::pairing(proof.final_a, proof.final_h);

    let check_t = t_prime == final_t;
    assert!(check_t == true);
    let check_u = u_prime == final_u;
    assert!(check_u == true);
    check_h & check_u & check_t
  }
}

/// compress is similar to commit::{V,W}KEY::compress: it modifies the `vec`
/// vector by setting the value at index $i:0 -> split$  $vec[i] = vec[i] +
/// vec[i+split]^scaler$. The `vec` vector is half of its size after this call.
pub fn compress<C: AffineCurve>(vec: &mut Vec<C>, split: usize, scaler: &C::ScalarField) {
  let (left, right) = vec.split_at_mut(split);
  left
    .par_iter_mut()
    .zip(right.par_iter())
    .for_each(|(a_l, a_r)| {
      // TODO remove that with master version
      let mut x = a_r.mul(scaler.into_repr());
      x.add_assign_mixed(&a_l);
      *a_l = x.into_affine();
    });
  let len = left.len();
  vec.resize(len, C::zero());
}

// TODO make that generic with points as well
pub fn compress_field<F: PrimeField>(vec: &mut Vec<F>, split: usize, scaler: &F) {
  let (left, right) = vec.split_at_mut(split);
  assert!(left.len() == right.len());
  left
    .par_iter_mut()
    .zip(right.par_iter_mut())
    .for_each(|(a_l, a_r)| {
      // TODO remove copy
      a_r.mul_assign(scaler);
      a_l.add_assign(a_r.clone());
    });
  let len = left.len();
  vec.resize(len, F::zero());
}

pub fn multiexponentiation<G: AffineCurve>(
  left: &[G],
  right: &[G::ScalarField],
) -> Result<G::Projective, Error> {
  if left.len() != right.len() {
    return Err(Error::InvalidIPVectorLength);
  }

  Ok(VariableBaseMSM::multi_scalar_mul(
    left,
    &cfg_iter!(right).map(|s| s.into_repr()).collect::<Vec<_>>(),
  ))
}

pub fn pairings_product<E: PairingEngine>(gs: &[E::G1Affine], hs: &[E::G2Affine]) -> E::Fqk {
  let pairings: Vec<_> = gs
    .into_par_iter()
    .map(|g| <E as PairingEngine>::G1Prepared::from(*g))
    .zip(
      hs.into_par_iter()
        .map(|h| <E as PairingEngine>::G2Prepared::from(*h)),
    )
    .collect();

  E::product_of_pairings(pairings.iter())
}

#[derive(Debug, Error)]
pub enum Error {
  #[error("Serialization error: {0}")]
  Serialization(#[from] SerializationError),

  #[error("Commitment key length invalid")]
  InvalidKeyLength,

  #[error("Vectors length do not match for inner product (IP)")]
  InvalidIPVectorLength,

  #[error("Invalid pairing result")]
  InvalidPairing,

  #[error("Invalid SRS: {0}")]
  InvalidSRS(String),

  #[error("Invalid proof: {0}")]
  InvalidProof(String),

  #[error("Malformed Groth16 verifying key")]
  MalformedVerifyingKey,
}

/// Transcript is the application level transcript to derive the challenges
/// needed for Fiat Shamir during aggregation. It is given to the
/// prover/verifier so that the transcript can be fed with any other data first.
/// TODO: Make this trait the only Transcript trait
pub trait Transcript {
  fn domain_sep(&mut self);
  fn append<S: CanonicalSerialize>(&mut self, label: &'static [u8], point: &S);
  fn challenge_scalar<F: PrimeField>(&mut self, label: &'static [u8]) -> F;
}

#[cfg(test)]
mod tests {
  use ark_bls12_377::{Bls12_377 as E, Fr};
  use ark_ec::PairingEngine;
  use ark_poly::DenseMultilinearExtension;
  use ark_poly_commit::multilinear_pc::MultilinearPC;
  use ark_std::{test_rng, UniformRand};
  #[test]
  fn test_setup() {
    let mut rng = test_rng();
    let params = MultilinearPC::<E>::setup(2, &mut rng);
    // list of evaluation for polynomial
    // 1 + 2*x_1 + x_2 + x_1x_2
    let evals_1 = vec![
      Fr::from(1u64),
      Fr::from(4u64),
      Fr::from(2u64),
      Fr::from(5u64),
    ];
    let poly_1 = DenseMultilinearExtension::<Fr>::from_evaluations_vec(2, evals_1);

    // list of evaluation for polynomial
    // 1 + x_1 + x_2 + 2*x_1x_2
    let evals_2 = vec![
      Fr::from(1u64),
      Fr::from(2u64),
      Fr::from(2u64),
      Fr::from(5u64),
    ];
    let poly_2 = DenseMultilinearExtension::<Fr>::from_evaluations_vec(2, evals_2);
  }
}
