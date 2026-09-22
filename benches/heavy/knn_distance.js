// Squared distances from 100000 query points to 128-dimensional centroids
function squared_distance(dim, query, centroid) {
    let s0 = 0.0;
    let s1 = 0.0;
    let s2 = 0.0;
    let s3 = 0.0;
    for (let i = 0; i < dim; i += 4) {
        let d = query + i * 0.001 - (centroid + i * 0.002);
        s0 = s0 + d * d;
        d = query + (i + 1) * 0.001 - (centroid + (i + 1) * 0.002);
        s1 = s1 + d * d;
        d = query + (i + 2) * 0.001 - (centroid + (i + 2) * 0.002);
        s2 = s2 + d * d;
        d = query + (i + 3) * 0.001 - (centroid + (i + 3) * 0.002);
        s3 = s3 + d * d;
    }
    return s0 + s1 + s2 + s3;
}

const dim = 128;
const queries = 100000;
const centroids = 24;
let best = 0.0;
for (let q = 0; q < queries; q++) {
    for (let c = 0; c < centroids; c++) {
        best = best + squared_distance(dim, q * 0.01, c * 0.1);
    }
}
console.log("knn distance total: " + best);