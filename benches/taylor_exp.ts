// Taylor series: compute e^x
function taylor_exp(x: number, terms: number): number {
    let sum: number = 0.0;
    let term: number = 1.0;
    for (let k: number = 1; k <= terms; k++) {
        sum = sum + term;
        term = term * x / k;
    }
    return sum;
}

const result: number = taylor_exp(10.0, 100000);
console.log("taylor exp: " + result);