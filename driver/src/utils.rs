use rand::Rng;

pub fn boolean_with_probability(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    rng.gen::<f64>() < probability
}