// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// #[test]
// fn test_array_variable_definitions() {
//     type_checker_vars_type_test(
//         "
//         let a1 [int; 3] = [10, 20, 30]
//         let a2 Array<String, 3> = [\"a\", \"b\", \"c\"]
//         let a3 Array<i128, 3> = [1, 2, 3]
//         let a4 Array<float, 3> = [1.1, 2.2, 3.3]
//         let a5 Array<f64, 3> = [1.5, 2.5, 3.5]
// ",
//         vec![
//             ("a1", type_array(type_int(), 3)),
//             ("a2", type_array(type_string(), 3)),
//             ("a3", type_array(type_i128(), 3)),
//             ("a4", type_array(type_float(), 3)),
//             ("a5", type_array(type_f64(), 3)),
//         ],
//     )
// }
