use assert_tv::{TestValue, TestVector, TestVectorNOP, TestVectorSet};


fn main() {
    let r = c1::<TestVectorNOP>(1, 2);
    println!("{:?}", r)
}

#[derive(TestVectorSet)]
struct Fields {
    m: TestValue<i32>
}

fn c1<TV: TestVector>(x1: i32, x2: i32) -> i32 {
    let fields: Fields = TV::initialize_values();
    let m: i32 = x1*5;
    let m: i32 = TV::expose_value(&fields.m, m);
    m - x2
}

#[cfg(test)]
mod tests {
    use assert_tv::{test_vec_case, TestValue, TestVectorActive, TestVectorSet, TestVector};
    use crate::{c1, main};

    #[test]
    fn it_works() {
        main();
    }

    #[derive(TestVectorSet)]
    struct TestFields {
        a: TestValue<i32>,
        #[test_vec(name="b", description="b is the second input", offload = true)]
        b: TestValue<i32>,
        output: TestValue<i32>
    }
    
    #[test_vec_case(mode="init")]
    fn test_vector_case_2() -> Result<(), String> {
        let setup_fields: TestFields = TestVectorActive::initialize_values();
        let a = TestVectorActive::expose_value(&setup_fields.a, 4);
        let b = TestVectorActive::expose_value(&setup_fields.b, 4);
        let output = c1::<TestVectorActive>(a, b);
        TestVectorActive::check_value(&setup_fields.output, &output);
        Ok(())
    }

}