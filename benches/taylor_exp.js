// Taylor series: compute e^x
function taylor_exp(x, terms) {
    let sum = 0.0;
    let term = 1.0;
    for (let k = 1; k <= terms; k++) {
        sum = sum + term;
        term = term * x / k;
    }
    return sum;
}

const result = taylor_exp(10.0, 100000);
console.log("taylor exp: " + result);