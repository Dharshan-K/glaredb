// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use crate::planner::{AsyncContextProvider, SqlQueryPlanner};
use async_recursion::async_recursion;
use datafusion::common::{DataFusionError, Result};
use datafusion::logical_expr::{LogicalPlan, LogicalPlanBuilder};
use datafusion::sql::planner::PlannerContext;
use datafusion::sql::sqlparser::ast::{SetExpr, SetOperator, SetQuantifier};

impl<'a, S: AsyncContextProvider> SqlQueryPlanner<'a, S> {
    #[async_recursion]
    pub(super) async fn set_expr_to_plan(
        &mut self,
        set_expr: SetExpr,
        planner_context: &mut PlannerContext,
    ) -> Result<LogicalPlan> {
        match set_expr {
            SetExpr::Select(s) => self.select_to_plan(*s, planner_context).await,
            SetExpr::Values(v) => self.sql_values_to_plan(v, planner_context).await,
            SetExpr::SetOperation {
                op,
                left,
                right,
                set_quantifier,
            } => {
                let all = match set_quantifier {
                    SetQuantifier::All => true,
                    SetQuantifier::Distinct | SetQuantifier::None => false,
                    SetQuantifier::ByName => {
                        return Err(DataFusionError::NotImplemented(
                            "UNION BY NAME not implemented".to_string(),
                        ));
                    }
                    SetQuantifier::AllByName => {
                        return Err(DataFusionError::NotImplemented(
                            "UNION ALL BY NAME not implemented".to_string(),
                        ))
                    }
                };

                let left_plan = self.set_expr_to_plan(*left, planner_context).await?;
                let right_plan = self.set_expr_to_plan(*right, planner_context).await?;
                match (op, all) {
                    (SetOperator::Union, true) => LogicalPlanBuilder::from(left_plan)
                        .union(right_plan)?
                        .build(),
                    (SetOperator::Union, false) => LogicalPlanBuilder::from(left_plan)
                        .union_distinct(right_plan)?
                        .build(),
                    (SetOperator::Intersect, true) => {
                        LogicalPlanBuilder::intersect(left_plan, right_plan, true)
                    }
                    (SetOperator::Intersect, false) => {
                        LogicalPlanBuilder::intersect(left_plan, right_plan, false)
                    }
                    (SetOperator::Except, true) => {
                        LogicalPlanBuilder::except(left_plan, right_plan, true)
                    }
                    (SetOperator::Except, false) => {
                        LogicalPlanBuilder::except(left_plan, right_plan, false)
                    }
                }
            }
            SetExpr::Query(q) => self.query_to_plan_with_context(*q, planner_context).await,
            _ => Err(DataFusionError::NotImplemented(format!(
                "Query {set_expr} not implemented yet"
            ))),
        }
    }
}
