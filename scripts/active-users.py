#!/usr/bin/env python3
"""Print name + email for Active users with a real (human) name.

Source data is the Google Workspace admin user list (pasted inline below),
NOT the dev-pulse API — dev-pulse has no Active/status field. Filters out:
  - non-Active users (Suspended / Archived)
  - role / shared mailboxes (Nube iO Support, NubeIO Alerts, No Reply, etc.)
"""

# Each entry: (name, email, status). status "Active" is kept; others dropped.
USERS = [
    ("Abish bhusal",          "abh@nube-io.com",        "Active"),
    ("Admin Vietnam",         "vdc@nube-io.com",        "Active"),
    ("Aidan Pickard",         "ap@nube-io.com",         "Active"),
    ("Aman Maharjan",         "ama@nube-io.com",        "Active"),
    ("Amy Crehan",            "acr@nube-io.com",        "Active"),
    ("Arya Poudel",           "apo@nube-io.com",        "Active"),
    ("Binh Nguyen",           "bng@nube-io.com",        "Suspended"),
    ("Binod Rai",             "br@nube-io.com",         "Active"),
    ("Brabeem Sapkota",       "bsa@nube-io.com",        "Active"),
    ("Bryn Jarman",           "bja@nube-io.com",        "Active"),
    ("Caseh Dela Cruz",       "cdc@nube-io.com",        "Active"),
    ("Charissa Nuncio",       "cnu@nube-io.com",        "Active"),
    ("Chau Ngoc Que",         "qcn@nube-io.com",        "Active"),
    ("Claire Lavell",         "cla@nube-io.com",        "Active"),
    ("Craig Borrows",         "cbo@nube-io.com",        "Active"),
    ("Daniel McKinnell",      "dmc@nube-io.com",        "Active"),
    ("Dev Team",              "dev@nube-io.com",        "Active"),
    ("Enju Rai",              "era@nube-io.com",        "Active"),
    ("Hoang Dang Hoai",       "hhd@nube-io.com",        "Active"),
    ("Janine Mae Bernardino", "jmb@nube-io.com",        "Archived"),
    ("Jenesh Pradhananga",    "jpr@nube-io.com",        "Active"),
    ("Jerry Gubatan",         "jgu@nube-io.com",        "Active"),
    ("Jia Fei",               "jfe@nube-io.com",        "Active"),
    ("Jon Kane",              "jka@nube-io.com",        "Active"),
    ("Jonathan Hill",         "jhi@nube-io.com",        "Active"),
    ("Kester Yau",            "kya@nube-io.com",        "Active"),
    ("Kristine Mae Vargas",   "kma@nube-io.com",        "Active"),
    ("Lina Silvera",          "marketing@nube-io.com",  "Active"),
    ("Luis Balan",            "lba@nube-io.com",        "Active"),
    ("Mai Anh Tran",          "mai.anh@nube-io.com",    "Active"),
    ("Manish KC",             "mkc@nube-io.com",        "Active"),
    ("Manjeet Pandey",        "mpa@nube-io.com",        "Active"),
    ("Matthew Cady",          "m.cady@nube-io.com",     "Active"),
    ("Nam Tran Phuong",       "nam.tran@nube-io.com",   "Active"),
    ("Nghia Mai",             "nghia.mai@nube-io.com",  "Active"),
    ("Nghia Mai Trong",       "ntr@nube-io.com",        "Active"),
    ("Niel Teves",            "nte@nube-io.com",        "Active"),
    ("No Reply",              "noreply@nube-io.com",    "Active"),
    ("Nube Admin",            "admin@nube-io.com",      "Active"),
    ("Nube iO Support",       "support@nube-io.com",    "Active"),
    ("Nube iO Accounts",      "accounts@nube-io.com",   "Active"),
    ("Nube iO Service",       "service@nube-io.com",    "Active"),
    ("Nube iO Info",          "info@nube-io.com",       "Active"),
    ("Nube iO Orders",        "orders@nube-io.com",     "Active"),
    ("NubeIO Alerts",         "alerts@nube-io.com",     "Active"),
    ("Paul Munoz",            "pmu@nube-io.com",        "Suspended"),
    ("Phuong Do Nam",         "phuong@nube-io.com",     "Suspended"),
    ("Ritesh Shakya",         "rsh@nube-io.com",        "Active"),
    ("Shaun Hosseinzadeh",    "sho@nube-io.com",        "Active"),
    ("Simon Mahoney",         "sma@nube-io.com",        "Active"),
]

# Names that are role/shared accounts, not real people.
NON_HUMAN = {
    "Admin Vietnam", "Dev Team", "No Reply", "Nube Admin",
    "Nube iO Support", "Nube iO Accounts", "Nube iO Service",
    "Nube iO Info", "Nube iO Orders", "NubeIO Alerts",
}


def is_human(name):
    if name in NON_HUMAN:
        return False
    low = name.lower()
    if low.startswith(("nube", "admin")):
        return False
    return True


def main():
    rows = [(n, e) for (n, e, s) in USERS if s == "Active" and is_human(n)]
    width = max(len(n) for n, _ in rows)
    for name, email in rows:
        print(f"{name:<{width}}  {email}")
    print(f"\n{len(rows)} active users")


if __name__ == "__main__":
    main()
