use color_print::cformat;
use num_format::{Locale, ToFormattedString};
use tosho_amap::AMClient;

use crate::{
    cli::ExitCode,
    config::{get_all_config, save_config, try_remove_config},
};

use super::{
    common::{make_client, select_single_account},
    config::Config,
};

pub async fn amap_account_login(
    email: String,
    password: String,
    console: &crate::term::Terminal,
) -> ExitCode {
    console.info(&cformat!(
        "Authenticating with email <m,s>{}</> and password <m,s>{}</>...",
        email,
        password
    ));

    let all_configs = get_all_config(crate::r#impl::Implementations::Amap, None);

    let old_config = all_configs.iter().find(|&c| match c {
        crate::config::ConfigImpl::Amap(cc) => cc.email == email,
        _ => false,
    });

    let mut old_id: Option<String> = None;
    if let Some(old_config) = old_config {
        console.warn("Email already authenticated!");
        let abort_it = console.confirm(Some("Do you want to replace it?"));
        if !abort_it {
            console.info("Aborting...");
            return 0;
        }

        match old_config {
            crate::config::ConfigImpl::Amap(c) => {
                old_id = Some(c.id.clone());
            }
            _ => unreachable!(),
        }
    }

    let result = AMClient::login(&email, &password).await;

    match result {
        Ok(session) => {
            console.info(&cformat!(
                "Authenticated as <m,s>{}</> ({})",
                session.identifier,
                email
            ));

            let client = super::common::make_client(&session);
            let account = client.get_account().await;

            let as_config: Config = session.into();

            match account {
                Ok(account) => {
                    let as_config = as_config
                        .with_email(&email)
                        .with_account_info(&account.info);

                    console.info(&cformat!("Logged in as <m,s>{}</>", account.info.name));

                    let final_config = match old_id {
                        Some(old_id) => as_config.with_id(&old_id),
                        None => as_config,
                    };

                    console.info(&cformat!(
                        "Created session ID <m,s>{}</>, saving config...",
                        final_config.id
                    ));

                    save_config(crate::config::ConfigImpl::Amap(final_config), None);

                    0
                }
                Err(e) => {
                    console.error(&format!("Failed to login: {}", e));
                    1
                }
            }
        }
        Err(e) => {
            console.error(&format!("Failed to authenticate: {}", e));
            1
        }
    }
}

pub(crate) fn amap_accounts(console: &crate::term::Terminal) -> ExitCode {
    let all_configs = get_all_config(crate::r#impl::Implementations::Amap, None);

    match all_configs.len() {
        0 => {
            console.warn("No accounts found!");

            1
        }
        _ => {
            console.info(&format!("Found {} accounts:", all_configs.len()));
            for (i, c) in all_configs.iter().enumerate() {
                match c {
                    crate::config::ConfigImpl::Amap(c) => {
                        let plat_name = c.r#type().to_name();
                        console.info(&cformat!(
                            "{:02}. {} — <s>{}</> ({})",
                            i + 1,
                            c.id,
                            c.email,
                            plat_name,
                        ));
                    }
                    _ => unreachable!(),
                }
            }

            0
        }
    }
}

pub(crate) async fn amap_account_info(
    account_id: Option<&str>,
    console: &crate::term::Terminal,
) -> ExitCode {
    let acc_info = select_single_account(account_id);

    match acc_info {
        None => {
            console.warn("Aborted!");

            1
        }
        Some(acc_info) => {
            console.info(&cformat!(
                "Fetching account info for <magenta,bold>{}</>...",
                acc_info.id
            ));

            let client = make_client(&acc_info.clone().into());
            let account = client.get_account().await;

            match account {
                Ok(account) => {
                    let info = account.info;

                    console.info(&cformat!(
                        "Account info for <magenta,bold>{}</>:",
                        acc_info.id
                    ));

                    console.info(&cformat!("  <s>ID</>: {}", info.id));
                    console.info(&cformat!("  <s>Email</>: {}", acc_info.email));
                    console.info(&cformat!("  <s>Username</>: {}", info.name));

                    0
                }
                Err(e) => {
                    console.error(&format!("Failed to fetch account info: {}", e));

                    1
                }
            }
        }
    }
}

pub(crate) async fn amap_account_balance(
    account_id: Option<&str>,
    console: &crate::term::Terminal,
) -> ExitCode {
    let acc_info = select_single_account(account_id);

    if acc_info.is_none() {
        console.warn("Aborted!");

        return 1;
    }

    let acc_info = acc_info.unwrap();

    let client = make_client(&acc_info.clone().into());

    console.info(&cformat!(
        "Fetching balance for <magenta,bold>{}</>...",
        acc_info.id
    ));
    let remainder = client.get_remainder().await;

    match remainder {
        Ok(remainder) => {
            let balance = &remainder.info;

            console.info("Your current point balance:");
            let total_ticket = balance.sum().to_formatted_string(&Locale::en);
            let purchased = balance.purchased.to_formatted_string(&Locale::en);
            let premium = balance.premium.to_formatted_string(&Locale::en);
            let total_point = balance.sum_point().to_formatted_string(&Locale::en);

            console.info(&cformat!(
                "  - <s>Total</>: <magenta,bold><reverse>{}</>T</magenta,bold>",
                total_ticket
            ));
            console.info(&cformat!(
                "  - <s>Purchased</>: <yellow,bold><reverse>{}</>T</yellow,bold>",
                purchased
            ));
            console.info(&cformat!(
                "  - <s>Premium</>: <green,bold><reverse>{}</>T</green,bold>",
                premium
            ));
            console.info(&cformat!(
                "  - <s>Total point</>: <cyan!,bold><reverse>{}</>p</cyan!,bold>",
                total_point
            ));

            0
        }
        Err(e) => {
            console.error(&format!("Failed to fetch balance: {}", e));

            1
        }
    }
}

pub(crate) fn amap_account_revoke(
    account_id: Option<&str>,
    console: &crate::term::Terminal,
) -> ExitCode {
    let account = select_single_account(account_id);

    if account.is_none() {
        console.warn("Aborted");
        return 1;
    }

    let account = account.unwrap();
    let confirm = console.confirm(Some(&cformat!(
        "Are you sure you want to delete <m,s>{}</>?\nThis action is irreversible!",
        account.id
    )));

    if !confirm {
        console.warn("Aborted");
        return 0;
    }

    match try_remove_config(
        account.id.as_str(),
        crate::r#impl::Implementations::Amap,
        None,
    ) {
        Ok(_) => {
            console.info(&cformat!(
                "Successfully deleted <magenta,bold>{}</>",
                account.id
            ));
            0
        }
        Err(err) => {
            console.error(&format!("Failed to delete account: {}", err));
            1
        }
    }
}
