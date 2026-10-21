# Publier l'extension NadsPDF

L'extension s'installe depuis deux stores :

- le **Chrome Web Store**, pour Chrome, Brave, Comet, Opera et les autres navigateurs Chromium ;
- **Edge Add-ons**, pour Microsoft Edge (Edge accepte aussi le Chrome Web Store, mais ses
  utilisateurs cherchent d'abord dans le sien).

Ce que les utilisateurs obtiennent est l'interface **simplifiée, en français** : lire, commenter,
remplir, signer, modifier le texte, les images et les pages, reconnaître le texte (OCR), enregistrer
et imprimer (`?lite=1&lang=fr`, voir `build.ps1`). Le paquet contient les modèles de l'OCR
(`cargo xtask models` avant de construire) ; la page ne les charge qu'à la première reconnaissance.

## Le piège de l'identifiant

La règle qui redirige les PDF vers NadsPDF contient l'identifiant de l'extension
(`chrome-extension://<id>/index.html`). Chaque store donne à l'extension **son propre
identifiant**, différent de celui de la version de développement. Le Chrome Web Store refuse en
plus un `manifest.json` qui contient une clé (`key`). Il faut donc, pour chaque store :

1. envoyer un premier paquet pour obtenir l'identifiant ;
2. reconstruire avec cet identifiant (`-Store … -ExtensionId …`) ;
3. remplacer le paquet par le nouveau **avant** de soumettre à la vérification.

Une extension publiée avec le mauvais identifiant s'installe, mais n'ouvre aucun PDF. Avant de
soumettre, vérifiez que l'identifiant affiché par le store figure bien dans `rules.json`.

## Préparer les fichiers une fois

- **Paquet** : construit par `build.ps1` (ci-dessous). Le fichier WebAssembly pèse environ 40 Mo,
  ce que les deux stores acceptent.
- **Icône** : `dist\chrome-extension\icons\128.png`.
- **Captures d'écran** : 1280 × 800 (ou 640 × 400), de 1 à 5. Montrez un PDF ouvert dans
  l'extension, un commentaire, un formulaire rempli, une signature. N'utilisez que des documents
  de démonstration, sans données personnelles ni contenu Adobe (`AGENTS.md` §1).
- **Petite vignette promotionnelle** : 440 × 280 (demandée par le Chrome Web Store).
- **Politique de confidentialité** : une adresse publique vers `PRIVACY.md`, par exemple
  `https://github.com/nadsous/nadspdf/blob/main/PRIVACY.md` si le dépôt est public, sinon une
  page de votre site.
- **Description courte** (132 caractères au plus) : « Ouvre les PDF dans le navigateur avec
  NadsPDF : lire, annoter, remplir, signer, modifier et OCR, sans rien envoyer sur Internet. »

## Chrome Web Store

1. **Compte développeur** : <https://chrome.google.com/webstore/devconsole>, connexion avec un
   compte Google, frais d'inscription uniques de 5 $.
2. **Premier envoi** : construisez un paquet provisoire, sans identifiant (`-FirstUpload` ; le
   script affiche un avertissement, c'est voulu, et refuse un `-Store` sans l'un ou l'autre) :

   ```powershell
   powershell -File packaging\chrome\build.ps1 -Store chrome -FirstUpload
   ```

   Cliquez sur **Nouvel élément** et envoyez `dist\release\nadspdf-<version>-chrome-store.zip`.
   Le brouillon créé affiche l'**identifiant** (32 lettres de a à p). Ne le soumettez pas encore.
3. **Paquet définitif** :

   ```powershell
   powershell -File packaging\chrome\build.ps1 -SkipBuild -Store chrome -ExtensionId <identifiant>
   ```

   Cette commande produit `dist\release\nadspdf-<version>-chrome-store.zip`. Dans le brouillon,
   ouvrez **Package › Upload new package** et envoyez-le.
4. **Fiche** (*Store listing*) : description, catégorie (*Productivité*), langue français, icône,
   captures et vignette.
5. **Confidentialité** (*Privacy*) :
   - *Objectif unique* : « Afficher et modifier dans le navigateur les PDF ouverts par
     l'utilisateur. »
   - `declarativeNetRequestWithHostAccess` : « Rediriger l'onglet qui ouvre un PDF vers le lecteur
     intégré à l'extension. »
   - *Autorisations d'hôte* (`<all_urls>`) : « Télécharger le PDF que l'utilisateur ouvre, quel que
     soit le site, pour l'afficher dans l'extension. Rien n'est envoyé ailleurs. »
   - *Code distant* : non (tout le code, WebAssembly compris, est dans le paquet).
   - *Utilisation des données* : ne cochez aucune catégorie, puis cochez les trois engagements.
   - Indiquez l'adresse de la politique de confidentialité.
6. **Distribution** : public, tous les pays (ou une liste).
7. **Soumettre pour vérification**. Avec l'accès à toutes les adresses, la vérification est plus
   poussée et prend souvent plusieurs jours.

## Edge Add-ons

1. **Compte** : <https://partner.microsoft.com/dashboard/microsoftedge/overview>, inscription
   gratuite au programme Microsoft Edge avec un compte Microsoft.
2. **Premier envoi** : **Créer une extension**, puis envoyez un paquet provisoire construit avec
   `build.ps1 -SkipBuild -Store edge -FirstUpload` (`dist\release\nadspdf-<version>-edge-store.zip`). Relevez l'identifiant attribué par Edge (*CRX ID*) dans l'aperçu de l'extension.
3. **Paquet définitif** :

   ```powershell
   powershell -File packaging\chrome\build.ps1 -SkipBuild -Store edge -ExtensionId <identifiant Edge>
   ```

   Remplacez ensuite le paquet par `dist\release\nadspdf-<version>-edge-store.zip`.
4. **Disponibilité**, **Propriétés** (catégorie *Productivité*, politique de confidentialité) et
   **Fiche du store** en français (description, logo, captures). Reprenez les mêmes textes que
   pour Chrome.
5. **Publier**. Microsoft annonce jusqu'à 7 jours ouvrés de vérification.

## Les versions suivantes

1. Augmentez `version` dans `Cargo.toml` à la racine : le store refuse une version déjà publiée.
   Pour renvoyer l'extension sans changer la version de l'application, ajoutez `-Revision 1`
   (puis 2, 3…) : le manifeste dit alors `0.7.6.1` et le zip s'appelle `nadspdf-0.7.6.1-chrome-store.zip`.
2. Reconstruisez avec le **même** `-ExtensionId` que la première fois : l'identifiant ne change
   plus.

   ```powershell
   powershell -File packaging\chrome\build.ps1 -Store chrome -ExtensionId <identifiant Chrome>
   powershell -File packaging\chrome\build.ps1 -SkipBuild -Store edge -ExtensionId <identifiant Edge>
   ```

3. Envoyez chaque zip dans son store (*Package › Upload new package* pour Chrome, *Packages*
   pour Edge), puis soumettez. Les navigateurs installent la mise à jour d'eux-mêmes.

## Vérifier avant d'envoyer

- Chargez `dist\chrome-extension` en mode développeur et ouvrez :
  - un lien `.pdf` ;
  - un PDF sans `.pdf` dans l'adresse (par exemple <https://arxiv.org/pdf/1706.03762>) ;
  - un PDF de l'ordinateur, après avoir activé *Autoriser l'accès aux URL de fichier*.
- L'interface doit être en français, sans les modes Modifier et Convertir. L'onglet doit porter
  le nom du fichier.
- Le paquet construit avec `-Store` porte l'identifiant du store : il ne redirige donc rien s'il
  est chargé en mode développeur. C'est normal.
