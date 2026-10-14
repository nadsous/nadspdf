# Publier NadsPDF sur le Microsoft Store

Le Store accepte NadsPDF sous forme de paquet **MSIX**. Le Store **signe lui-même** le paquet :
pas besoin de certificat de signature de code. Une fois l'application publiée, le Store installe
les mises à jour automatiquement chez les utilisateurs (NadsPDF détecte qu'il tourne depuis
`C:\Program Files\WindowsApps\` et ne cherche alors plus de mises à jour lui-même).

## 1. Réserver le nom

1. Ouvrez <https://partner.microsoft.com/dashboard> ▸ **Applications et jeux** ▸
   **Nouveau produit** ▸ **Application MSIX ou PWA**.
2. Saisissez **NadsPDF** et cliquez sur **Vérifier la disponibilité**, puis **Réserver le nom
   du produit**.

## 2. Copier l'identité du paquet (une seule fois)

1. Dans la page de l'application : **Gestion des produits** ▸ **Identité du produit**.
2. Ouvrez `packaging/windows/msix/store-identity.psd1` et remplacez les trois valeurs
   `REMPLACER-...` :

   | Dans Partner Center                       | Dans `store-identity.psd1` |
   |-------------------------------------------|----------------------------|
   | `Package/Identity/Name`                   | `IdentityName`             |
   | `Package/Identity/Publisher` (`CN=...`)   | `Publisher`                |
   | `Package/Properties/PublisherDisplayName` | `PublisherDisplayName`     |

   Copiez-les à l'identique. Elles ne sont pas secrètes (elles figurent dans chaque paquet), le
   fichier peut donc être commité.

## 3. Construire le paquet

Dans PowerShell, à la racine du dépôt :

```powershell
# 1) binaires + modèles OCR (si ce n'est pas déjà fait pour cette version)
powershell -ExecutionPolicy Bypass -File packaging\windows\package.ps1 -Arch x64
# 2) le paquet Store
powershell -ExecutionPolicy Bypass -File packaging\windows\msix\build-msix.ps1
```

Résultat : `dist\release\nadspdf-<version>-windows-x64.msix` (non signé, c'est normal).

- La version vient de `Cargo.toml` (`0.7.1` devient `0.7.1.0`). Chaque soumission doit avoir
  une version **plus grande** que la précédente : augmentez la version avant de republier.
- `-Test` construit un paquet d'essai (`...-test.msix`) avec une identité factice, sans
  toucher à `store-identity.psd1`. Il ne doit pas être envoyé au Store.
- Facultatif : le kit de certification (Windows App Certification Kit,
  `C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe`) permet de lancer
  en local les mêmes contrôles que le Store. Le Store les refait de toute façon.

## 4. Créer la soumission

Dans Partner Center ▸ NadsPDF ▸ **Démarrer votre soumission** :

1. **Tarification et disponibilité** : marchés (tous), visibilité publique, **Gratuit**.
2. **Propriétés** :
   - Catégorie : **Productivité** (sous-catégorie facultative).
   - **URL de la politique de confidentialité** (obligatoire, car l'application a accès à
     Internet) :
     `https://github.com/nadsous/nadspdf/blob/main/PRIVACY.md`
     (le fichier `PRIVACY.md` doit être poussé sur `main` avant la soumission).
   - Site web / support : `https://github.com/nadsous/nadspdf` et
     `https://github.com/nadsous/nadspdf/issues`.
   - Configuration requise : clavier et souris recommandés.
3. **Classifications par âge** : remplissez le questionnaire IARC. Réponses attendues :
   pas de contenu violent, sexuel, de jeu d'argent, pas de partage de données ni d'interaction
   entre utilisateurs, pas d'achats. Résultat attendu : **3+ / Tout public**.
4. **Packages** : glissez `nadspdf-<version>-windows-x64.msix`. Le Store vérifie l'identité ;
   une erreur ici signifie presque toujours une valeur mal copiée à l'étape 2.
   Familles d'appareils : cochez **Windows 10/11 Desktop** uniquement.
5. **Fiches du Store** : ajoutez une fiche **Français (France)** et une **Anglais
   (États-Unis)** (le paquet déclare ces deux langues). Pour chacune :
   - Description (texte proposé ci-dessous).
   - **Captures d'écran** : au moins 1, **1366 × 768 ou plus** (1920 × 1080 conseillé), au
     format PNG. Montrez un PDF ouvert, l'annotation, la conversion, la signature.
   - Logo du Store facultatif (le paquet contient déjà les icônes).
   - Fonctionnalités clés (jusqu'à 20 lignes courtes), mots-clés : PDF, éditeur PDF, signer,
     annoter, convertir, OCR, fusionner.
6. **Options de soumission** ▸ **Capacités restreintes** : le Store demande pourquoi
   l'application utilise `runFullTrust`. Collez ce texte :

   > NadsPDF est une application de bureau Win32 native (écrite en Rust) empaquetée en MSIX.
   > La capacité runFullTrust est nécessaire pour exécuter cet exécutable Win32 : elle est
   > requise pour toute application de bureau empaquetée (point d'entrée
   > Windows.FullTrustApplication). L'application l'utilise pour ouvrir, modifier et
   > enregistrer les fichiers PDF que l'utilisateur choisit, aux emplacements qu'il choisit, et
   > pour la reconnaissance de texte (OCR) entièrement locale. Elle ne collecte aucune donnée.

   English version, if the form is in English:

   > NadsPDF is a native Win32 desktop application (written in Rust) packaged as MSIX.
   > runFullTrust is required to run this Win32 executable, as for every packaged desktop app
   > (Windows.FullTrustApplication entry point). It is used to open, edit and save the PDF
   > files the user picks, wherever the user keeps them, and for fully local OCR. The app
   > collects no data.

7. Cliquez sur **Soumettre au Store**. La certification prend en général de quelques heures à
   3 jours ouvrés. Partner Center envoie un e-mail en cas de refus, avec le motif.

### Description proposée (fiche française)

> NadsPDF est un éditeur PDF complet, rapide et gratuit, qui fonctionne entièrement sur votre
> ordinateur : vos documents ne quittent jamais votre appareil.
>
> - Lire et rechercher dans vos PDF, même volumineux
> - Annoter, surligner, commenter, dessiner
> - Modifier le texte et les images, organiser, fusionner, découper et faire pivoter les pages
> - Remplir et signer des formulaires, signer numériquement
> - Convertir Word, Excel, images, texte et HTML en PDF
> - Reconnaissance de texte (OCR) locale pour les documents numérisés
> - Aucun compte, aucune publicité, aucune collecte de données

## 5. Mises à jour suivantes

1. Lancez `powershell -ExecutionPolicy Bypass -File packaging\release.ps1 X.Y.Z` (un numéro plus
   grand) : il change la version, construit le `.msi`, l'extension et le `.msix`, et publie la
   release GitHub (que les copies installées avec le `.msi` proposent en « Mettre à jour »).
2. Partner Center ▸ NadsPDF ▸ **Mettre à jour** la soumission ▸ **Packages** : remplacez le
   `.msix`, puis soumettez. Le Store met à jour les utilisateurs automatiquement.

## Ce que la version Store fait différemment de l'installateur MSI

- **Menu contextuel de l'Explorateur** : seuls « Ouvrir avec NadsPDF » et « Fusionner avec
  NadsPDF » sur les fichiers `.pdf` sont présents (sous « Afficher plus d'options » sur
  Windows 11). Les entrées « Convertir en PDF avec NadsPDF » sur les fichiers Word, Excel,
  images, etc. de l'installateur MSI demanderaient une extension COM (IExplorerCommand) que
  NadsPDF n'a pas. L'option des préférences qui ajoute ces entrées écrit dans le registre de
  l'utilisateur ; dans un paquet MSIX ces écritures restent privées à l'application et
  l'Explorateur ne les voit pas.
- **Raccourci sur le bureau** : non créé (les paquets MSIX n'en créent pas) ; l'application est
  dans le menu Démarrer et peut y être épinglée.
- **Ligne de commande** : `nadspdf.exe` est disponible dans le terminal (alias d'exécution).
  `pdfcraft-cli.exe` est inclus dans le paquet mais n'a pas d'alias.
- **Association .pdf** : NadsPDF apparaît dans « Ouvrir avec » et dans Paramètres ▸
  Applications par défaut, sans jamais s'imposer comme lecteur par défaut.
