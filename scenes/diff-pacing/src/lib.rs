//! Stepped Diff pacing showroom: a method gains a currency. Its braces leave
//! and arrive in the same step, so paced marks show `+` only on the rows that
//! changed, and the marks fade once read. Then `DiffEditor::focus` lights two
//! rows and dims the rest, with waves on a lit row and on a dimmed one; the
//! next step's new row arrives lit while the light moves to it; `unfocus`
//! lights every row again.
use anyhow::Result;
use psychopomp::{
    author::{PlanBuilder, seconds},
    editor::diff::{ChangeMarks, Diff, add, keep, remove},
    ide::{DiagnosticActor, DiagnosticPlan},
    plan::ScenePlan,
};

/// Line indices in the diff below.
const SIGNATURE: usize = 6;
const MONEY: usize = 8;
const PAY: usize = 9;
const NOTE: usize = 12;

/// `paced` declares with [`ChangeMarks::paced`]; without it the same diff
/// marks every re-added row and keeps its tint, as `Diff::declare` does.
pub fn build(paced: bool) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new("diff-pacing", seconds(11.0));
    let at = seconds;
    let diff = Diff {
        file_name: "Checkout.cs",
        lines: vec![
            keep("public class Checkout(IPayment payment)"),
            keep("{"),
            remove(1, "    public string PlaceOrder(decimal total)"),
            remove(1, "    {"),
            remove(1, "        return payment.Pay(total);"),
            remove(1, "    }"),
            add(
                1,
                "    public string PlaceOrder(decimal total, string currency)",
            )
            .range("currency", "string currency"),
            add(1, "    {"),
            add(1, "        var amount = Money.Of(total, currency);"),
            add(1, "        return payment.Pay(amount);").range("pay", "payment.Pay(amount)"),
            add(1, "    }"),
            keep("}"),
            add(2, "// Callers pass the currency now."),
        ],
    };
    let steps = [at(1.5), at(6.5)];
    let editor = if paced {
        diff.declare_paced(&mut scene, &steps, 0, false, ChangeMarks::paced())?
    } else {
        diff.declare(&mut scene, &steps, 0, false)?
    };
    editor.target(&mut scene, "currency", SIGNATURE, "currency")?;
    editor.target(&mut scene, "pay", PAY, "pay")?;

    // Light the new signature and its use; a wave on the lit row, and one on
    // a dimmed row that keeps its full strength.
    editor.focus(&mut scene, &[SIGNATURE, MONEY], at(4.0))?;
    let currency = DiagnosticActor::declare(
        &mut scene,
        "currency-hl",
        &DiagnosticPlan::info("currency").without_gutter(),
    )?;
    currency.show(&mut scene, at(4.3));
    currency.clear(&mut scene, at(6.2));
    let pay = DiagnosticActor::declare(
        &mut scene,
        "pay-hl",
        &DiagnosticPlan::warning("pay").without_gutter(),
    )?;
    pay.show(&mut scene, at(4.6));
    pay.clear(&mut scene, at(6.2));

    // The next step's row arrives lit as the light moves to it.
    editor.focus(&mut scene, &[NOTE], at(6.5))?;
    editor.unfocus(&mut scene, at(9.5));
    Ok(scene.finish()?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_editions_build() {
        super::build(true).unwrap();
        super::build(false).unwrap();
    }
}
