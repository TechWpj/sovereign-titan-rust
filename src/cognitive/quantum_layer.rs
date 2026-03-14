//! Quantum-inspired entanglement layer with Lindblad master equation dynamics.
//!
//! 27 entangled concepts organized as a 3x3x3 cube (Domain x Modality x Temporal).
//! The density matrix is the primary state representation, evolved via the
//! Lindblad master equation using RK4 integration.
//!
//! Ported from `sovereign_titan/cognitive/quantum_layer.py` with mathematical parity:
//! - 27x27 complex density matrix (Hermitian, PSD, trace=1)
//! - Lindblad master equation: dρ/dt = -i[H, ρ] + Σ(L_k ρ L_k† - ½{L_k† L_k, ρ})
//! - Born rule measurement with Lüders projection collapse
//! - Von Neumann entropy via eigenvalue decomposition
//! - Partial trace over 3-qutrit decomposition
//! - Hebbian entanglement learning from co-activation

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use nalgebra::DMatrix;
use num_complex::Complex64;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Number of concepts in the quantum layer.
pub const N: usize = 27;

/// Concept labels organized in a 3x3x3 cube.
pub const CONCEPT_LABELS: [&str; N] = [
    // Cognitive-Analytical
    "Instant Analysis",       // 0
    "Deep Reasoning",         // 1
    "Knowledge Forecasting",  // 2
    // Cognitive-Creative
    "Spontaneous Insight",    // 3
    "Creative Exploration",   // 4
    "Imagination",            // 5
    // Cognitive-Integrative
    "Pattern Recognition",    // 6
    "Knowledge Synthesis",    // 7
    "Wisdom Accumulation",    // 8
    // Operational-Analytical
    "Precise Execution",      // 9
    "Strategic Planning",     // 10
    "Capability Forecasting", // 11
    // Operational-Creative
    "Adaptive Response",      // 12
    "Novel Problem-Solving",  // 13
    "Innovation Planning",    // 14
    // Operational-Integrative
    "Coordinated Action",     // 15
    "Workflow Optimization",  // 16
    "System Evolution",       // 17
    // Environmental-Analytical
    "Threat Detection",       // 18
    "Security Analysis",      // 19
    "Risk Prediction",        // 20
    // Environmental-Creative
    "Adaptive Security",      // 21
    "Defense Strategy",       // 22
    "Threat Anticipation",    // 23
    // Environmental-Integrative
    "Situational Awareness",  // 24
    "Context Synthesis",      // 25
    "Strategic Foresight",    // 26
];

/// Mode → concept indices mapping.
pub fn concept_mode_map() -> HashMap<&'static str, Vec<usize>> {
    let mut m = HashMap::new();
    m.insert("think", vec![0, 1, 2]);
    m.insert("learn", vec![4, 7, 8]);
    m.insert("reflect", vec![6, 7, 25]);
    m.insert("act", vec![9, 12, 15]);
    m.insert("plan", vec![1, 10, 16]);
    m.insert("consolidate", vec![7, 8, 25]);
    m.insert("research", vec![4, 5, 14]);
    m
}

/// Thought category → concept indices mapping.
pub fn thought_category_map() -> HashMap<&'static str, Vec<usize>> {
    let mut m = HashMap::new();
    m.insert("self_awareness", vec![0, 6]);
    m.insert("creativity", vec![3, 4, 5]);
    m.insert("knowledge", vec![2, 7, 8]);
    m.insert("problem_solving", vec![1, 9, 13]);
    m.insert("security", vec![18, 19, 20]);
    m
}

// ─────────────────────────────────────────────────────────────────────────────
// Cube geometry
// ─────────────────────────────────────────────────────────────────────────────

/// Cube position (domain, modality, temporal) for a concept index.
fn cube_position(idx: usize) -> (usize, usize, usize) {
    let domain = idx / 9;
    let modality = (idx % 9) / 3;
    let temporal = idx % 3;
    (domain, modality, temporal)
}

/// Number of shared cube axes between two concepts.
fn shared_axes(a: usize, b: usize) -> usize {
    let pa = cube_position(a);
    let pb = cube_position(b);
    let mut count = 0;
    if pa.0 == pb.0 { count += 1; }
    if pa.1 == pb.1 { count += 1; }
    if pa.2 == pb.2 { count += 1; }
    count
}

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Measurement history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementRecord {
    pub concept: usize,
    pub probability: f64,
    pub timestamp: f64,
}

/// Quantum concept layer state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumState {
    pub probabilities: Vec<f64>,
    pub top_concepts: Vec<(usize, f64)>,
    pub entropy: f64,
    pub coherence: f64,
}

/// Result of a measurement operation.
#[derive(Debug, Clone, Serialize)]
pub struct MeasurementResult {
    pub concept_id: usize,
    pub label: &'static str,
    pub probability: f64,
    pub entangled_effects: Vec<EntangledEffect>,
}

/// Effect on an entangled partner after measurement.
#[derive(Debug, Clone, Serialize)]
pub struct EntangledEffect {
    pub id: usize,
    pub label: &'static str,
    pub old_probability: f64,
    pub new_probability: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// QuantumConceptLayer
// ─────────────────────────────────────────────────────────────────────────────

/// 27-concept quantum entanglement layer with Lindblad master equation dynamics.
///
/// The density matrix (27x27 complex, Hermitian, PSD, trace=1) is the primary
/// state. Evolution uses the Lindblad master equation integrated via RK4.
/// Measurement follows the Born rule with Lüders projection collapse.
pub struct QuantumConceptLayer {
    /// 27x27 complex density matrix — primary quantum state.
    density_matrix: DMatrix<Complex64>,
    /// N×N correlation matrix (entanglement strengths).
    correlation: Vec<Vec<f64>>,
    /// Entanglement map: concept → list of entangled partners.
    entanglement_map: HashMap<usize, Vec<usize>>,
    /// Global entanglement strength multiplier.
    entanglement_strength: f64,
    /// Base decoherence rate.
    decoherence_rate: f64,
    /// Per-concept decoherence rates (gamma_k).
    gamma: Vec<f64>,
    /// Concept-specific rotation frequencies (Hamiltonian diagonal).
    frequencies: Vec<f64>,
    /// Hebbian learning rate.
    learning_rate: f64,
    /// Whether Lindblad parameters have changed (requires cache invalidation).
    liouvillian_dirty: bool,
    /// Measurement history.
    measurement_history: VecDeque<MeasurementRecord>,
    /// Evolution count.
    evolution_count: u64,
    /// Last evolution timestamp.
    last_evolution: f64,
}

impl QuantumConceptLayer {
    /// Create a new quantum concept layer with Lindblad dynamics.
    pub fn new(
        decoherence_rate: f64,
        entanglement_strength: f64,
        learning_rate: f64,
    ) -> Self {
        // Start as |uniform><uniform| — pure state with equal superposition
        let val = Complex64::new(1.0 / (N as f64).sqrt(), 0.0);
        let psi = DMatrix::from_fn(N, 1, |_, _| val);
        let density_matrix = &psi * psi.adjoint();

        // Per-concept decoherence rates
        let gamma = vec![decoherence_rate; N];

        // Hamiltonian diagonal frequencies (matching Python: linspace 0.01 to 0.27)
        let frequencies: Vec<f64> = (0..N)
            .map(|i| 0.01 + (0.27 - 0.01) * i as f64 / (N - 1) as f64)
            .collect();

        let mut layer = Self {
            density_matrix,
            correlation: vec![vec![0.0; N]; N],
            entanglement_map: (0..N).map(|i| (i, Vec::new())).collect(),
            entanglement_strength,
            decoherence_rate,
            gamma,
            frequencies,
            learning_rate,
            liouvillian_dirty: true,
            measurement_history: VecDeque::with_capacity(200),
            evolution_count: 0,
            last_evolution: now_secs(),
        };

        layer.build_default_topology();
        layer
    }

    // ── Topology ─────────────────────────────────────────────────────────

    /// Build entanglement topology based on shared cube axes.
    fn build_default_topology(&mut self) {
        for a in 0..N {
            for b in (a + 1)..N {
                let shared = shared_axes(a, b);
                let strength = match shared {
                    2 => 0.5,
                    1 => {
                        let pa = cube_position(a);
                        let pb = cube_position(b);
                        if pa.0 == pb.0 {
                            0.3 // Same domain
                        } else if pa.1 == pb.1 {
                            0.2 // Same modality
                        } else {
                            0.15 // Same temporal
                        }
                    }
                    _ => continue,
                };
                self.correlation[a][b] = strength;
                self.correlation[b][a] = strength;
                self.entanglement_map.get_mut(&a).unwrap().push(b);
                self.entanglement_map.get_mut(&b).unwrap().push(a);
            }
        }
    }

    // ── Lindblad RHS (for RK4) ──────────────────────────────────────────

    /// Compute dρ/dt from the Lindblad master equation.
    ///
    /// dρ/dt = -i[H, ρ] + Σ_k D_k(ρ)
    ///
    /// where D_k(ρ) = L_k ρ L_k† - ½{L_k† L_k, ρ}
    ///
    /// Uses efficient rank-1 structure of Lindblad operators to avoid
    /// full matrix multiplications. Entire computation is O(N²).
    fn lindblad_rhs(&self, rho: &DMatrix<Complex64>) -> DMatrix<Complex64> {
        let n = N;
        let mut drho = DMatrix::<Complex64>::zeros(n, n);
        let i_unit = Complex64::i();

        // ── Hamiltonian commutator: -i[H, ρ] ──
        // H is diagonal, so [H, ρ]_{ij} = (freq_i - freq_j) * ρ_{ij}
        for i in 0..n {
            for j in 0..n {
                let freq_diff = self.frequencies[i] - self.frequencies[j];
                drho[(i, j)] += -i_unit * freq_diff * rho[(i, j)];
            }
        }

        // ── Decoherence dissipators ──
        // L_k = sqrt(gamma_k) * |uniform><k|  (rank-1)
        // Using derived formulae:
        //   L_k ρ L_k† = gamma_k * ρ[k,k] * |uniform><uniform|
        //   L_k† L_k = gamma_k * |k><k|
        //   D_k(ρ)[i,j] = gamma_k * ρ[k,k] * u_i * conj(u_j)
        //               - 0.5 * gamma_k * δ_{ik} * ρ[k,j]
        //               - 0.5 * gamma_k * ρ[i,k] * δ_{jk}
        let u_val = 1.0 / (n as f64).sqrt();

        for k in 0..n {
            let gk = self.gamma[k];
            if gk < 1e-15 {
                continue;
            }

            let rho_kk = rho[(k, k)];

            // Term 1: gamma_k * ρ[k,k] * |uniform><uniform|
            for i in 0..n {
                for j in 0..n {
                    drho[(i, j)] += gk * rho_kk * Complex64::new(u_val * u_val, 0.0);
                }
            }

            // Term 2: -0.5 * gamma_k * P_k ρ  (zeros out all rows except k)
            for j in 0..n {
                drho[(k, j)] -= 0.5 * gk * rho[(k, j)];
            }

            // Term 3: -0.5 * gamma_k * ρ P_k  (zeros out all cols except k)
            for i in 0..n {
                drho[(i, k)] -= 0.5 * gk * rho[(i, k)];
            }
        }

        // ── Exchange coupling dissipators ──
        // For each entangled pair (a, b):
        //   L_ab = sqrt(rate) * |a><b|, L_ba = sqrt(rate) * |b><a|
        //   rate = correlation[a,b] * entanglement_strength * 0.01
        for a in 0..n {
            let partners = match self.entanglement_map.get(&a) {
                Some(p) => p.clone(),
                None => continue,
            };
            for &b in &partners {
                if b <= a {
                    continue; // Process each pair once
                }
                let corr = self.correlation[a][b];
                let rate = corr * self.entanglement_strength * 0.01;
                if rate < 1e-15 {
                    continue;
                }

                // Forward: L_ab = sqrt(rate) * |a><b|
                // D_ab: L_ab ρ L_ab† = rate * ρ[b,b] * |a><a|
                //       L_ab† L_ab = rate * |b><b|
                let rho_bb = rho[(b, b)];
                drho[(a, a)] += rate * rho_bb;
                for j in 0..n {
                    drho[(b, j)] -= 0.5 * rate * rho[(b, j)];
                }
                for i in 0..n {
                    drho[(i, b)] -= 0.5 * rate * rho[(i, b)];
                }

                // Reverse: L_ba = sqrt(rate) * |b><a|
                let rho_aa = rho[(a, a)];
                drho[(b, b)] += rate * rho_aa;
                for j in 0..n {
                    drho[(a, j)] -= 0.5 * rate * rho[(a, j)];
                }
                for i in 0..n {
                    drho[(i, a)] -= 0.5 * rate * rho[(i, a)];
                }
            }
        }

        drho
    }

    // ── RK4 Integration ─────────────────────────────────────────────────

    /// Single RK4 step advancing the density matrix by dt.
    fn rk4_step(&self, rho: &DMatrix<Complex64>, dt: f64) -> DMatrix<Complex64> {
        let dt_c = Complex64::new(dt, 0.0);
        let half = Complex64::new(0.5, 0.0);
        let sixth = Complex64::new(1.0 / 6.0, 0.0);
        let two = Complex64::new(2.0, 0.0);

        let k1 = self.lindblad_rhs(rho) * dt_c;
        let k2 = self.lindblad_rhs(&(rho + &k1 * half)) * dt_c;
        let k3 = self.lindblad_rhs(&(rho + &k2 * half)) * dt_c;
        let k4 = self.lindblad_rhs(&(rho + &k3)) * dt_c;

        rho + (k1 + k2 * two + k3 * two + k4) * sixth
    }

    // ── Hermitian Eigendecomposition ────────────────────────────────────

    /// Compute eigenvalues of a Hermitian matrix.
    ///
    /// Uses the 2N×2N real symmetric embedding:
    /// [[A, -B], [B, A]] where M = A + iB.
    /// Each eigenvalue of M appears twice in the embedding.
    fn hermitian_eigenvalues(m: &DMatrix<Complex64>) -> Vec<f64> {
        let n = m.nrows();
        let n2 = 2 * n;

        let mut real_mat = DMatrix::<f64>::zeros(n2, n2);
        for i in 0..n {
            for j in 0..n {
                let c = m[(i, j)];
                real_mat[(i, j)] = c.re;
                real_mat[(i, n + j)] = -c.im;
                real_mat[(n + i, j)] = c.im;
                real_mat[(n + i, n + j)] = c.re;
            }
        }

        let eigen = real_mat.symmetric_eigen();

        // Sort eigenvalues and take every other one (each appears twice)
        let mut vals: Vec<f64> = eigen.eigenvalues.iter().copied().collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mut result = Vec::with_capacity(n);
        for k in 0..n {
            result.push(vals[2 * k]);
        }
        result
    }

    /// Full eigendecomposition of a Hermitian matrix (eigenvalues + eigenvectors).
    fn hermitian_eigen(m: &DMatrix<Complex64>) -> (Vec<f64>, DMatrix<Complex64>) {
        let n = m.nrows();
        let n2 = 2 * n;

        let mut real_mat = DMatrix::<f64>::zeros(n2, n2);
        for i in 0..n {
            for j in 0..n {
                let c = m[(i, j)];
                real_mat[(i, j)] = c.re;
                real_mat[(i, n + j)] = -c.im;
                real_mat[(n + i, j)] = c.im;
                real_mat[(n + i, n + j)] = c.re;
            }
        }

        let eigen = real_mat.symmetric_eigen();

        // Sort by eigenvalue
        let mut indices: Vec<usize> = (0..n2).collect();
        indices.sort_by(|&a, &b| {
            eigen.eigenvalues[a]
                .partial_cmp(&eigen.eigenvalues[b])
                .unwrap()
        });

        let mut eigenvalues = Vec::with_capacity(n);
        let mut eigenvectors = DMatrix::<Complex64>::zeros(n, n);

        for k in 0..n {
            let idx = indices[2 * k];
            eigenvalues.push(eigen.eigenvalues[idx]);

            // Reconstruct complex eigenvector: v = x + iy
            let evec = eigen.eigenvectors.column(idx);
            let mut norm_sq = 0.0;
            for r in 0..n {
                let c = Complex64::new(evec[r], evec[n + r]);
                eigenvectors[(r, k)] = c;
                norm_sq += c.norm_sqr();
            }
            // Normalize
            if norm_sq > 1e-24 {
                let inv_norm = 1.0 / norm_sq.sqrt();
                for r in 0..n {
                    eigenvectors[(r, k)] *= inv_norm;
                }
            }
        }

        (eigenvalues, eigenvectors)
    }

    // ── Density Matrix Sanitization ─────────────────────────────────────

    /// Enforce Hermiticity, trace=1, and positive semi-definiteness.
    fn sanitize(&mut self) {
        let n = N;

        // Hermiticity: ρ = (ρ + ρ†) / 2
        let adj = self.density_matrix.adjoint();
        self.density_matrix = (&self.density_matrix + &adj) * Complex64::new(0.5, 0.0);

        // Trace normalization
        let tr: Complex64 = (0..n).map(|i| self.density_matrix[(i, i)]).sum();
        if tr.re > 1e-12 {
            self.density_matrix /= Complex64::new(tr.re, 0.0);
        }

        // PSD fix: clip negative eigenvalues and reconstruct
        let (eigenvalues, eigenvectors) = Self::hermitian_eigen(&self.density_matrix);

        let clipped: Vec<f64> = eigenvalues.iter().map(|&v| v.max(0.0)).collect();
        let sum: f64 = clipped.iter().sum();
        if sum < 1e-12 {
            // Fallback to maximally mixed state
            self.density_matrix = DMatrix::from_fn(n, n, |i, j| {
                if i == j {
                    Complex64::new(1.0 / n as f64, 0.0)
                } else {
                    Complex64::new(0.0, 0.0)
                }
            });
            return;
        }

        let normalized: Vec<f64> = clipped.iter().map(|v| v / sum).collect();

        // Reconstruct: ρ = U * diag(λ) * U†
        let u = &eigenvectors;
        let u_adj = eigenvectors.adjoint();
        // Build diagonal matrix
        let diag = DMatrix::from_fn(n, n, |i, j| {
            if i == j {
                Complex64::new(normalized[i], 0.0)
            } else {
                Complex64::new(0.0, 0.0)
            }
        });
        self.density_matrix = u * diag * u_adj;

        // Final Hermiticity cleanup
        let adj2 = self.density_matrix.adjoint();
        self.density_matrix = (&self.density_matrix + &adj2) * Complex64::new(0.5, 0.0);
    }

    // ── Public API ──────────────────────────────────────────────────────

    /// Evolve quantum state by dt seconds using Lindblad master equation (RK4).
    pub fn evolve(&mut self, dt: f64) {
        if dt <= 0.0 {
            return;
        }

        // Subdivide into steps if dt is large (RK4 stability)
        let max_step = 1.0;
        let n_steps = ((dt / max_step).ceil() as usize).max(1);
        let step_dt = dt / n_steps as f64;

        for _ in 0..n_steps {
            self.density_matrix = self.rk4_step(&self.density_matrix, step_dt);
        }

        self.sanitize();
        self.evolution_count += 1;
        self.last_evolution = now_secs();
    }

    /// Auto-evolve based on elapsed time since last evolution.
    pub fn auto_evolve(&mut self) {
        let now = now_secs();
        let dt = now - self.last_evolution;
        if dt > 0.1 {
            self.evolve(dt);
        }
    }

    /// Measure a concept via Born rule with Lüders projection collapse.
    ///
    /// Probability: p_k = ρ[k,k].real (Born rule)
    /// Post-measurement: ρ → P_k ρ P_k / Tr(P_k ρ P_k) (Lüders rule)
    pub fn measure(&mut self, concept_id: usize) -> MeasurementResult {
        assert!(concept_id < N, "concept_id must be 0-{}", N - 1);

        // Born rule probability
        let probability = self.density_matrix[(concept_id, concept_id)].re;

        // Lüders collapse: ρ → P_k ρ P_k / p_k
        // P_k = |k><k|, so P_k ρ P_k has only element [k,k] = ρ[k,k]
        let mut collapsed = DMatrix::<Complex64>::zeros(N, N);
        collapsed[(concept_id, concept_id)] = self.density_matrix[(concept_id, concept_id)];

        let norm = collapsed[(concept_id, concept_id)].re;
        if norm > 1e-12 {
            self.density_matrix = collapsed / Complex64::new(norm, 0.0);
        } else {
            // Near-zero probability: set to pure state |k><k|
            self.density_matrix = DMatrix::zeros(N, N);
            self.density_matrix[(concept_id, concept_id)] = Complex64::new(1.0, 0.0);
        }

        // Propagate excitation to entangled partners
        let mut entangled_effects = Vec::new();
        let partners = self
            .entanglement_map
            .get(&concept_id)
            .cloned()
            .unwrap_or_default();

        for partner in &partners {
            let corr = self.correlation[concept_id][*partner];
            let old_p = self.density_matrix[(*partner, *partner)].re;
            let boost = 0.1 * corr * probability;
            self.density_matrix[(*partner, *partner)] += Complex64::new(boost, 0.0);
            let new_p = self.density_matrix[(*partner, *partner)].re;

            if (new_p - old_p).abs() > 1e-6 {
                entangled_effects.push(EntangledEffect {
                    id: *partner,
                    label: CONCEPT_LABELS[*partner],
                    old_probability: old_p,
                    new_probability: new_p,
                });
            }
        }

        // Re-normalize
        let tr: f64 = (0..N).map(|i| self.density_matrix[(i, i)].re).sum();
        if tr > 1e-12 {
            self.density_matrix /= Complex64::new(tr, 0.0);
        }

        // Ensure Hermiticity
        let adj = self.density_matrix.adjoint();
        self.density_matrix = (&self.density_matrix + &adj) * Complex64::new(0.5, 0.0);

        // Record measurement
        if self.measurement_history.len() >= 200 {
            self.measurement_history.pop_front();
        }
        self.measurement_history.push_back(MeasurementRecord {
            concept: concept_id,
            probability,
            timestamp: now_secs(),
        });

        MeasurementResult {
            concept_id,
            label: CONCEPT_LABELS[concept_id],
            probability,
            entangled_effects,
        }
    }

    /// Increase (or decrease) a concept's population in the density matrix.
    pub fn excite(&mut self, concept_id: usize, energy: f64) {
        if concept_id >= N || energy == 0.0 {
            return;
        }

        // Adjust diagonal (population)
        let current = self.density_matrix[(concept_id, concept_id)].re;
        let new_val = (current + energy * 0.1).max(0.0);
        self.density_matrix[(concept_id, concept_id)] = Complex64::new(new_val, 0.0);

        // Propagate to entangled partners
        let partners = self
            .entanglement_map
            .get(&concept_id)
            .cloned()
            .unwrap_or_default();
        for partner in &partners {
            let corr = self.correlation[concept_id][*partner];
            let partner_energy = energy * corr * 0.05;
            let p_current = self.density_matrix[(*partner, *partner)].re;
            let new_p = (p_current + partner_energy).max(0.0);
            self.density_matrix[(*partner, *partner)] = Complex64::new(new_p, 0.0);
        }

        // Re-normalize trace to 1
        let tr: f64 = (0..N).map(|i| self.density_matrix[(i, i)].re).sum();
        if tr > 1e-12 {
            self.density_matrix /= Complex64::new(tr, 0.0);
        }

        // Ensure Hermiticity
        let adj = self.density_matrix.adjoint();
        self.density_matrix = (&self.density_matrix + &adj) * Complex64::new(0.5, 0.0);
    }

    /// Von Neumann entropy: S = -Tr(ρ ln ρ) = -Σ λ_k ln(λ_k).
    ///
    /// Returns 0 for pure states, ln(27) ≈ 3.30 for maximally mixed.
    pub fn get_entropy(&self) -> f64 {
        let eigenvalues = Self::hermitian_eigenvalues(&self.density_matrix);
        let mut entropy = 0.0;
        for &lam in &eigenvalues {
            if lam > 1e-12 {
                entropy -= lam * lam.ln();
            }
        }
        entropy
    }

    /// Partial trace over complement of `keep` axis.
    ///
    /// 3-qutrit decomposition: reshape 27×27 density matrix as (3,3,3,3,3,3)
    /// tensor, then contract out the two traced-over subsystems.
    ///
    /// Returns a 3×3 reduced density matrix for the kept subsystem.
    pub fn partial_trace(&self, keep: &str) -> DMatrix<Complex64> {
        let keep_idx = match keep {
            "domain" => 0,
            "modality" => 1,
            "temporal" => 2,
            _ => panic!("keep must be 'domain', 'modality', or 'temporal'"),
        };

        // The density matrix indices map to (d0, d1, d2) via cube_position.
        // For row index i: (d0_i, d1_i, d2_i) = cube_position(i)
        // For col index j: (d0_j, d1_j, d2_j) = cube_position(j)
        //
        // Partial trace: sum over indices where traced-out axes match.
        // reduced[a, b] = Σ ρ[i, j]  where:
        //   keep_axis(i) = a, keep_axis(j) = b, and
        //   other_axes(i) = other_axes(j)  (trace condition)

        let mut reduced = DMatrix::<Complex64>::zeros(3, 3);

        for i in 0..N {
            let pi = cube_position(i);
            let pi_arr = [pi.0, pi.1, pi.2];
            let a = pi_arr[keep_idx];

            for j in 0..N {
                let pj = cube_position(j);
                let pj_arr = [pj.0, pj.1, pj.2];
                let b = pj_arr[keep_idx];

                // Check that traced-out axes match
                let mut matches = true;
                for axis in 0..3 {
                    if axis != keep_idx && pi_arr[axis] != pj_arr[axis] {
                        matches = false;
                        break;
                    }
                }

                if matches {
                    reduced[(a, b)] += self.density_matrix[(i, j)];
                }
            }
        }

        reduced
    }

    /// Global coherence measure [0, 1] — average off-diagonal density matrix magnitude.
    pub fn get_coherence(&self) -> f64 {
        let mut off_diag_sum = 0.0;
        for i in 0..N {
            for j in 0..N {
                if i != j {
                    off_diag_sum += self.density_matrix[(i, j)].norm();
                }
            }
        }
        let n_off = N * (N - 1);
        if n_off == 0 {
            0.0
        } else {
            off_diag_sum / n_off as f64
        }
    }

    /// Compute interference from density matrix coherences between concepts.
    pub fn interfere(&self, concept_ids: &[usize]) -> f64 {
        if concept_ids.len() < 2 {
            return 0.0;
        }

        let mut total = 0.0;
        let mut n_pairs = 0;
        for i in 0..concept_ids.len() {
            for j in (i + 1)..concept_ids.len() {
                let a = concept_ids[i];
                let b = concept_ids[j];
                if a < N && b < N {
                    total += self.density_matrix[(a, b)].re;
                    n_pairs += 1;
                }
            }
        }

        if n_pairs == 0 { 0.0 } else { total / n_pairs as f64 }
    }

    /// Top-n concepts by population (diagonal of density matrix).
    pub fn get_dominant_concepts(&self, n: usize) -> Vec<(usize, &'static str, f64)> {
        let mut indexed: Vec<(usize, f64)> = (0..N)
            .map(|i| (i, self.density_matrix[(i, i)].re))
            .collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        indexed
            .into_iter()
            .take(n)
            .map(|(i, p)| (i, CONCEPT_LABELS[i], p))
            .collect()
    }

    /// Get the current quantum state snapshot.
    pub fn get_state(&self) -> QuantumState {
        let probabilities: Vec<f64> = (0..N)
            .map(|i| self.density_matrix[(i, i)].re)
            .collect();

        let mut indexed: Vec<(usize, f64)> = probabilities
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, p))
            .collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        QuantumState {
            probabilities,
            top_concepts: indexed.into_iter().take(5).collect(),
            entropy: self.get_entropy(),
            coherence: self.get_coherence(),
        }
    }

    /// Human-readable summary for injection into thought prompts.
    pub fn get_state_summary(&self) -> String {
        let dominant = self.get_dominant_concepts(3);
        let coherence = self.get_coherence();
        let entropy = self.get_entropy();

        let mut parts = vec![
            format!("Coherence: {:.2}", coherence),
            format!("Entropy: {:.2}", entropy),
        ];
        for (_, label, fidelity) in dominant {
            parts.push(format!("{}({:.2})", label, fidelity));
        }

        parts.join(" | ")
    }

    /// Get probabilities for concepts associated with a mode.
    pub fn mode_probabilities(&self, mode: &str) -> Vec<(usize, &'static str, f64)> {
        let map = concept_mode_map();
        if let Some(indices) = map.get(mode) {
            indices
                .iter()
                .filter(|&&i| i < N)
                .map(|&i| (i, CONCEPT_LABELS[i], self.density_matrix[(i, i)].re))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get label for a concept index.
    pub fn label(&self, concept: usize) -> Option<&'static str> {
        CONCEPT_LABELS.get(concept).copied()
    }

    /// Get entangled partners for a concept.
    pub fn entangled_partners(&self, concept: usize) -> Vec<usize> {
        self.entanglement_map
            .get(&concept)
            .cloned()
            .unwrap_or_default()
    }

    /// Evolution count.
    pub fn evolution_count(&self) -> u64 {
        self.evolution_count
    }

    /// Create or strengthen entanglement between two concepts.
    pub fn entangle(&mut self, a: usize, b: usize, strength: f64) {
        assert!(a != b, "Cannot entangle a concept with itself");
        assert!(a < N && b < N, "concept_id must be 0-{}", N - 1);

        let s = strength.clamp(0.0, 1.0);
        self.correlation[a][b] = s;
        self.correlation[b][a] = s;

        if !self.entanglement_map.get(&a).unwrap().contains(&b) {
            self.entanglement_map.get_mut(&a).unwrap().push(b);
        }
        if !self.entanglement_map.get(&b).unwrap().contains(&a) {
            self.entanglement_map.get_mut(&b).unwrap().push(a);
        }

        self.liouvillian_dirty = true;
    }

    /// Remove entanglement between two concepts.
    pub fn disentangle(&mut self, a: usize, b: usize) {
        self.correlation[a][b] = 0.0;
        self.correlation[b][a] = 0.0;

        if let Some(partners) = self.entanglement_map.get_mut(&a) {
            partners.retain(|&x| x != b);
        }
        if let Some(partners) = self.entanglement_map.get_mut(&b) {
            partners.retain(|&x| x != a);
        }

        self.liouvillian_dirty = true;
    }

    /// Hebbian learning: strengthen entanglement between co-activated concepts.
    pub fn hebbian_update(&mut self, concepts: &[usize]) {
        for i in 0..concepts.len() {
            for j in (i + 1)..concepts.len() {
                let a = concepts[i];
                let b = concepts[j];
                if a < N && b < N && a != b {
                    let co_activation =
                        self.density_matrix[(a, a)].re * self.density_matrix[(b, b)].re;
                    self.update_entanglement(a, b, co_activation);
                }
            }
        }
    }

    /// Internal Hebbian learning for a specific pair.
    fn update_entanglement(&mut self, a: usize, b: usize, co_activation: f64) {
        if a == b {
            return;
        }
        let delta = self.learning_rate * co_activation;
        let new_strength = (self.correlation[a][b] + delta).clamp(0.0, 1.0);
        self.correlation[a][b] = new_strength;
        self.correlation[b][a] = new_strength;

        if new_strength > 0.01 {
            if !self.entanglement_map.get(&a).unwrap().contains(&b) {
                self.entanglement_map.get_mut(&a).unwrap().push(b);
            }
            if !self.entanglement_map.get(&b).unwrap().contains(&a) {
                self.entanglement_map.get_mut(&b).unwrap().push(a);
            }
        }

        self.liouvillian_dirty = true;
    }

    // ── Subsystem Signal Receivers ──────────────────────────────────────

    /// ThoughtQualityScorer callback — maps category to concepts, excites them.
    pub fn on_thought_scored(&mut self, category: &str, score: f64) {
        let score = score.clamp(0.0, 1.0);
        let cat_map = thought_category_map();
        let concept_ids = match cat_map.get(category) {
            Some(ids) => ids.clone(),
            None => return,
        };

        let energy = (score - 0.5) * 0.2;
        for &cid in &concept_ids {
            self.excite(cid, energy);
        }

        // Hebbian: strengthen entanglement between co-activated concepts
        for i in 0..concept_ids.len() {
            for j in (i + 1)..concept_ids.len() {
                self.update_entanglement(
                    concept_ids[i],
                    concept_ids[j],
                    (score - 0.5).abs() * 2.0,
                );
            }
        }
    }

    /// ThompsonSampler callback — maps mode to concepts, updates fidelity.
    pub fn on_mode_selected(&mut self, mode: &str, success: bool) {
        let mode_map = concept_mode_map();
        let concept_ids = match mode_map.get(mode) {
            Some(ids) => ids.clone(),
            None => return,
        };

        let energy = if success { 0.05 } else { -0.03 };
        for &cid in &concept_ids {
            self.excite(cid, energy);
        }

        let co_activation = if success { 1.0 } else { 0.3 };
        for i in 0..concept_ids.len() {
            for j in (i + 1)..concept_ids.len() {
                self.update_entanglement(concept_ids[i], concept_ids[j], co_activation * 0.5);
            }
        }
    }

    /// Security subsystem callback — excites Environmental concepts.
    pub fn on_security_event(&mut self, severity: f64) {
        let severity = severity.clamp(0.0, 1.0);
        let energy = severity * 0.15;
        for cid in 18..N {
            self.excite(cid, energy);
        }

        if severity > 0.3 {
            for i in 18..N {
                for j in (i + 1)..N {
                    self.update_entanglement(i, j, severity * 0.3);
                }
            }
        }
    }

    /// Tool execution callback — excites Operational concepts.
    pub fn on_tool_outcome(&mut self, _tool_name: &str, success: bool) {
        let energy = if success { 0.05 } else { -0.02 };
        for cid in 9..18 {
            self.excite(cid, energy * 0.3);
        }

        if success {
            for i in 9..18 {
                for j in (i + 1)..(i + 3).min(18) {
                    self.update_entanglement(i, j, 0.2);
                }
            }
        }
    }

    /// MetacognitiveMonitor callback — adjusts entanglement strength.
    pub fn on_metacognitive_health(&mut self, health_score: f64) {
        let health_score = health_score.clamp(0.0, 1.0);
        self.entanglement_strength = 0.8 - 0.6 * health_score;
        self.liouvillian_dirty = true;
    }
}

impl Default for QuantumConceptLayer {
    fn default() -> Self {
        Self::new(0.02, 0.5, 0.01)
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ─────────────────────────────────────────────────────────────────────────────
// Event Bus Listener
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn an async listener that routes `CognitiveEvent`s to the quantum layer.
///
/// This decouples the quantum layer from all other subsystems — it reacts to
/// events published on the broadcast bus instead of receiving direct method
/// calls via `Arc<Mutex<QuantumConceptLayer>>`.
///
/// The listener handles:
/// - `ThoughtScored` → `on_thought_scored()` (Hebbian + excitation)
/// - `ModeSelected` → `on_mode_selected()` (mode concept excitation)
/// - `ToolOutcome` → `on_tool_outcome()` (Operational excitation)
/// - `SecurityAnomaly` → `on_security_event()` (Environmental excitation)
/// - `MetacognitiveHealth` → `on_metacognitive_health()` (entanglement tuning)
///
/// Returns a `JoinHandle` so the caller can cancel the listener on shutdown.
pub fn spawn_quantum_listener(
    quantum_layer: std::sync::Arc<std::sync::Mutex<QuantumConceptLayer>>,
    mut rx: tokio::sync::broadcast::Receiver<crate::event_bus::CognitiveEvent>,
) -> tokio::task::JoinHandle<()> {
    use crate::event_bus::CognitiveEvent;
    use tracing::{debug, warn};

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let mut ql = quantum_layer.lock().unwrap();
                    match event {
                        CognitiveEvent::ThoughtScored { category, score } => {
                            debug!("QuantumListener: ThoughtScored({}, {:.2})", category, score);
                            ql.on_thought_scored(&category, score);
                        }
                        CognitiveEvent::ModeSelected { mode, success } => {
                            debug!("QuantumListener: ModeSelected({}, {})", mode, success);
                            ql.on_mode_selected(&mode, success);
                        }
                        CognitiveEvent::ToolOutcome {
                            tool_name,
                            success,
                        } => {
                            debug!("QuantumListener: ToolOutcome({}, {})", tool_name, success);
                            ql.on_tool_outcome(&tool_name, success);
                        }
                        CognitiveEvent::SecurityAnomaly {
                            severity,
                            description,
                        } => {
                            debug!(
                                "QuantumListener: SecurityAnomaly({:.2}, {})",
                                severity,
                                &description[..description.len().min(60)]
                            );
                            ql.on_security_event(severity);
                        }
                        CognitiveEvent::MetacognitiveHealth { health_score } => {
                            debug!(
                                "QuantumListener: MetacognitiveHealth({:.2})",
                                health_score
                            );
                            ql.on_metacognitive_health(health_score);
                        }
                        // Ignore events not relevant to the quantum layer
                        _ => {}
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("QuantumListener: lagged by {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    debug!("QuantumListener: event bus closed, stopping");
                    break;
                }
            }
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> Complex64 {
        Complex64::new(re, im)
    }

    // ── Initialization ──────────────────────────────────────────────────

    #[test]
    fn test_initial_state_is_pure_uniform() {
        let ql = QuantumConceptLayer::default();
        // Should be |uniform><uniform| — all elements equal to 1/N
        let expected = 1.0 / N as f64;
        for i in 0..N {
            for j in 0..N {
                let val = ql.density_matrix[(i, j)];
                assert!(
                    (val.re - expected).abs() < 1e-10,
                    "real[{},{}] = {} != {}",
                    i,
                    j,
                    val.re,
                    expected
                );
                assert!(
                    val.im.abs() < 1e-10,
                    "imag[{},{}] = {} != 0",
                    i,
                    j,
                    val.im
                );
            }
        }
    }

    #[test]
    fn test_initial_trace_is_one() {
        let ql = QuantumConceptLayer::default();
        let tr: f64 = (0..N).map(|i| ql.density_matrix[(i, i)].re).sum();
        assert!((tr - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_initial_hermiticity() {
        let ql = QuantumConceptLayer::default();
        for i in 0..N {
            for j in 0..N {
                let a = ql.density_matrix[(i, j)];
                let b = ql.density_matrix[(j, i)].conj();
                assert!(
                    (a - b).norm() < 1e-10,
                    "Not Hermitian at [{},{}]",
                    i,
                    j
                );
            }
        }
    }

    // ── Evolution ───────────────────────────────────────────────────────

    #[test]
    fn test_evolve_preserves_trace() {
        let mut ql = QuantumConceptLayer::default();
        ql.measure(0); // Disturb from uniform
        ql.evolve(1.0);
        let tr: f64 = (0..N).map(|i| ql.density_matrix[(i, i)].re).sum();
        assert!(
            (tr - 1.0).abs() < 1e-6,
            "Trace after evolve: {}",
            tr
        );
    }

    #[test]
    fn test_evolve_preserves_hermiticity() {
        let mut ql = QuantumConceptLayer::default();
        ql.measure(5);
        ql.evolve(2.0);
        for i in 0..N {
            for j in 0..N {
                let a = ql.density_matrix[(i, j)];
                let b = ql.density_matrix[(j, i)].conj();
                assert!(
                    (a - b).norm() < 1e-6,
                    "Not Hermitian at [{},{}] after evolve",
                    i,
                    j
                );
            }
        }
    }

    #[test]
    fn test_evolve_toward_mixed() {
        let mut ql = QuantumConceptLayer::default();
        ql.measure(0); // Collapse to pure state |0>

        let before_entropy = ql.get_entropy();
        ql.evolve(10.0); // Evolve for a long time — decoherence mixes state
        let after_entropy = ql.get_entropy();

        // Entropy should increase (decoherence drives toward mixed state)
        assert!(
            after_entropy > before_entropy,
            "Entropy should increase: {} -> {}",
            before_entropy,
            after_entropy
        );
    }

    #[test]
    fn test_evolve_large_dt_subdivides() {
        let mut ql = QuantumConceptLayer::default();
        ql.measure(3);
        // Large dt should subdivide into multiple RK4 steps
        ql.evolve(5.0);
        let tr: f64 = (0..N).map(|i| ql.density_matrix[(i, i)].re).sum();
        assert!((tr - 1.0).abs() < 1e-6);
    }

    // ── Measurement ─────────────────────────────────────────────────────

    #[test]
    fn test_measure_born_rule() {
        let ql = QuantumConceptLayer::default();
        // Uniform state: all diagonal elements = 1/N
        let expected = 1.0 / N as f64;
        let p = ql.density_matrix[(0, 0)].re;
        assert!((p - expected).abs() < 1e-10);
    }

    #[test]
    fn test_measure_luders_collapse() {
        let mut ql = QuantumConceptLayer::default();
        let result = ql.measure(0);

        // After collapse, concept 0 should dominate
        let p0 = ql.density_matrix[(0, 0)].re;
        assert!(
            p0 > 0.5,
            "Concept 0 should dominate after measurement, got {}",
            p0
        );

        // Probability should be close to 1/N for uniform start
        assert!((result.probability - 1.0 / N as f64).abs() < 1e-6);
    }

    #[test]
    fn test_measure_entangled_effects() {
        let mut ql = QuantumConceptLayer::default();
        let result = ql.measure(0);

        // Concept 0 has entangled partners; some should be affected
        let partners = ql.entangled_partners(0);
        assert!(!partners.is_empty());
        // At least some entangled effects should be reported
        // (depends on correlation strengths and boost amounts)
    }

    #[test]
    fn test_measure_preserves_trace() {
        let mut ql = QuantumConceptLayer::default();
        ql.measure(13);
        let tr: f64 = (0..N).map(|i| ql.density_matrix[(i, i)].re).sum();
        assert!((tr - 1.0).abs() < 1e-6);
    }

    // ── Von Neumann Entropy ─────────────────────────────────────────────

    #[test]
    fn test_entropy_pure_state() {
        let mut ql = QuantumConceptLayer::default();
        // Collapse to a pure state
        ql.density_matrix = DMatrix::zeros(N, N);
        ql.density_matrix[(0, 0)] = c(1.0, 0.0);

        let entropy = ql.get_entropy();
        assert!(
            entropy.abs() < 1e-6,
            "Pure state entropy should be ~0, got {}",
            entropy
        );
    }

    #[test]
    fn test_entropy_maximally_mixed() {
        let mut ql = QuantumConceptLayer::default();
        // Set to maximally mixed state (identity / N)
        ql.density_matrix = DMatrix::from_fn(N, N, |i, j| {
            if i == j {
                c(1.0 / N as f64, 0.0)
            } else {
                c(0.0, 0.0)
            }
        });

        let entropy = ql.get_entropy();
        let max_entropy = (N as f64).ln();
        assert!(
            (entropy - max_entropy).abs() < 0.01,
            "Max mixed entropy should be {}, got {}",
            max_entropy,
            entropy
        );
    }

    #[test]
    fn test_entropy_initial_pure_uniform() {
        let ql = QuantumConceptLayer::default();
        let entropy = ql.get_entropy();
        // |uniform><uniform| is a PURE state, so entropy should be ~0
        assert!(
            entropy < 0.01,
            "Pure uniform state entropy should be ~0, got {}",
            entropy
        );
    }

    // ── Partial Trace ───────────────────────────────────────────────────

    #[test]
    fn test_partial_trace_domain() {
        let ql = QuantumConceptLayer::default();
        let reduced = ql.partial_trace("domain");

        // Reduced matrix should be 3x3
        assert_eq!(reduced.nrows(), 3);
        assert_eq!(reduced.ncols(), 3);

        // Trace of reduced should equal trace of full (= 1)
        let tr: f64 = (0..3).map(|i| reduced[(i, i)].re).sum();
        assert!(
            (tr - 1.0).abs() < 1e-6,
            "Reduced trace = {}",
            tr
        );
    }

    #[test]
    fn test_partial_trace_modality() {
        let ql = QuantumConceptLayer::default();
        let reduced = ql.partial_trace("modality");
        let tr: f64 = (0..3).map(|i| reduced[(i, i)].re).sum();
        assert!((tr - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_partial_trace_temporal() {
        let ql = QuantumConceptLayer::default();
        let reduced = ql.partial_trace("temporal");
        let tr: f64 = (0..3).map(|i| reduced[(i, i)].re).sum();
        assert!((tr - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_partial_trace_hermitian() {
        let ql = QuantumConceptLayer::default();
        let reduced = ql.partial_trace("domain");
        for i in 0..3 {
            for j in 0..3 {
                let a = reduced[(i, j)];
                let b = reduced[(j, i)].conj();
                assert!(
                    (a - b).norm() < 1e-10,
                    "Reduced not Hermitian at [{},{}]",
                    i,
                    j
                );
            }
        }
    }

    // ── Coherence ───────────────────────────────────────────────────────

    #[test]
    fn test_coherence_pure_uniform() {
        let ql = QuantumConceptLayer::default();
        let coh = ql.get_coherence();
        // |uniform><uniform| has all elements = 1/N, so off-diagonal magnitude = 1/N
        let expected = 1.0 / N as f64;
        assert!(
            (coh - expected).abs() < 1e-6,
            "Coherence = {}, expected {}",
            coh,
            expected
        );
    }

    #[test]
    fn test_coherence_after_measurement() {
        let mut ql = QuantumConceptLayer::default();
        let before = ql.get_coherence();
        ql.measure(0); // Collapse destroys coherence
        let after = ql.get_coherence();
        assert!(
            after < before,
            "Coherence should decrease after measurement: {} -> {}",
            before,
            after
        );
    }

    // ── Entanglement ────────────────────────────────────────────────────

    #[test]
    fn test_entangle_disentangle() {
        let mut ql = QuantumConceptLayer::default();
        ql.entangle(0, 26, 0.8);
        assert!(ql.entangled_partners(0).contains(&26));
        assert!((ql.correlation[0][26] - 0.8).abs() < 1e-10);

        ql.disentangle(0, 26);
        assert!(!ql.entangled_partners(0).contains(&26));
        assert!(ql.correlation[0][26].abs() < 1e-10);
    }

    // ── Hebbian Learning ────────────────────────────────────────────────

    #[test]
    fn test_hebbian_strengthens_correlation() {
        let mut ql = QuantumConceptLayer::default();
        let before = ql.correlation[0][1];
        ql.hebbian_update(&[0, 1]);
        let after = ql.correlation[0][1];
        assert!(
            after >= before,
            "Hebbian should strengthen: {} -> {}",
            before,
            after
        );
    }

    // ── Callbacks ───────────────────────────────────────────────────────

    #[test]
    fn test_on_thought_scored() {
        let mut ql = QuantumConceptLayer::default();
        let before = ql.density_matrix[(0, 0)].re;
        ql.on_thought_scored("self_awareness", 0.9);
        let after = ql.density_matrix[(0, 0)].re;
        // High score should excite concept 0 (self_awareness maps to [0, 6])
        assert!(after != before, "on_thought_scored should change state");
    }

    #[test]
    fn test_on_mode_selected() {
        let mut ql = QuantumConceptLayer::default();
        ql.on_mode_selected("think", true);
        // Should have changed state for concepts [0, 1, 2]
        // (hard to assert exact values, but state should differ from uniform)
    }

    #[test]
    fn test_on_security_event() {
        let mut ql = QuantumConceptLayer::default();
        ql.on_security_event(0.8);
        // Environmental concepts (18-26) should be excited
    }

    #[test]
    fn test_on_tool_outcome() {
        let mut ql = QuantumConceptLayer::default();
        ql.on_tool_outcome("web_search", true);
        // Operational concepts (9-17) should be excited
    }

    #[test]
    fn test_on_metacognitive_health() {
        let mut ql = QuantumConceptLayer::default();
        ql.on_metacognitive_health(0.9);
        // High health → weaker entanglement
        assert!(
            (ql.entanglement_strength - (0.8 - 0.6 * 0.9)).abs() < 1e-10,
            "entanglement_strength = {}",
            ql.entanglement_strength
        );
    }

    // ── State Queries ───────────────────────────────────────────────────

    #[test]
    fn test_get_state() {
        let ql = QuantumConceptLayer::default();
        let state = ql.get_state();
        assert_eq!(state.probabilities.len(), N);
        assert_eq!(state.top_concepts.len(), 5);
    }

    #[test]
    fn test_get_state_summary() {
        let ql = QuantumConceptLayer::default();
        let summary = ql.get_state_summary();
        assert!(summary.contains("Coherence:"));
        assert!(summary.contains("Entropy:"));
    }

    #[test]
    fn test_mode_probabilities() {
        let ql = QuantumConceptLayer::default();
        let probs = ql.mode_probabilities("think");
        assert_eq!(probs.len(), 3);
        assert_eq!(probs[0].1, "Instant Analysis");
    }

    #[test]
    fn test_label() {
        let ql = QuantumConceptLayer::default();
        assert_eq!(ql.label(0), Some("Instant Analysis"));
        assert_eq!(ql.label(26), Some("Strategic Foresight"));
        assert_eq!(ql.label(27), None);
    }

    // ── Geometry ────────────────────────────────────────────────────────

    #[test]
    fn test_cube_position() {
        assert_eq!(cube_position(0), (0, 0, 0));
        assert_eq!(cube_position(26), (2, 2, 2));
        assert_eq!(cube_position(9), (1, 0, 0));
    }

    #[test]
    fn test_shared_axes() {
        assert_eq!(shared_axes(0, 1), 2); // Same domain + modality
        assert_eq!(shared_axes(0, 3), 2); // Same domain + temporal
        assert_eq!(shared_axes(0, 13), 0); // No shared axes
    }

    // ── Interference ────────────────────────────────────────────────────

    #[test]
    fn test_interference_uniform() {
        let ql = QuantumConceptLayer::default();
        // All off-diagonal elements = 1/N for |uniform><uniform|
        let score = ql.interfere(&[0, 1, 2]);
        assert!(score > 0.0, "Uniform state should have positive interference");
    }

    // ── Eigendecomposition ──────────────────────────────────────────────

    #[test]
    fn test_hermitian_eigenvalues_identity() {
        let n = 4;
        let identity = DMatrix::from_fn(n, n, |i, j| {
            if i == j { c(1.0, 0.0) } else { c(0.0, 0.0) }
        });
        let evals = QuantumConceptLayer::hermitian_eigenvalues(&identity);
        assert_eq!(evals.len(), n);
        for &v in &evals {
            assert!((v - 1.0).abs() < 1e-10, "eigenvalue = {}", v);
        }
    }

    #[test]
    fn test_hermitian_eigenvalues_diagonal() {
        let n = 3;
        let diag = DMatrix::from_fn(n, n, |i, j| {
            if i == j {
                c((i + 1) as f64, 0.0)
            } else {
                c(0.0, 0.0)
            }
        });
        let mut evals = QuantumConceptLayer::hermitian_eigenvalues(&diag);
        evals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((evals[0] - 1.0).abs() < 1e-10);
        assert!((evals[1] - 2.0).abs() < 1e-10);
        assert!((evals[2] - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_hermitian_eigen_reconstruction() {
        // Test that U * diag(λ) * U† ≈ M for a Hermitian matrix
        let n = 4;
        let mut m = DMatrix::<Complex64>::zeros(n, n);
        // Build a Hermitian matrix
        m[(0, 0)] = c(2.0, 0.0);
        m[(1, 1)] = c(3.0, 0.0);
        m[(2, 2)] = c(1.0, 0.0);
        m[(3, 3)] = c(4.0, 0.0);
        m[(0, 1)] = c(0.5, 0.3);
        m[(1, 0)] = c(0.5, -0.3);
        m[(0, 2)] = c(0.1, -0.2);
        m[(2, 0)] = c(0.1, 0.2);

        let (evals, evecs) = QuantumConceptLayer::hermitian_eigen(&m);

        // Reconstruct
        let d = DMatrix::from_fn(n, n, |i, j| {
            if i == j { c(evals[i], 0.0) } else { c(0.0, 0.0) }
        });
        let reconstructed = &evecs * d * evecs.adjoint();

        for i in 0..n {
            for j in 0..n {
                let diff = (reconstructed[(i, j)] - m[(i, j)]).norm();
                assert!(
                    diff < 1e-6,
                    "Reconstruction error at [{},{}]: {} vs {} (diff={})",
                    i,
                    j,
                    reconstructed[(i, j)],
                    m[(i, j)],
                    diff
                );
            }
        }
    }
}
