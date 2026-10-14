# Identité Microsoft Store de NadsPDF, lue par build-msix.ps1.
#
# À remplir UNE fois. Les trois valeurs se trouvent dans Partner Center :
#   https://partner.microsoft.com/dashboard  >  Applications et jeux  >  NadsPDF
#   >  Gestion des produits  >  Identité du produit
#
# Copiez-les exactement (majuscules, points et tirets compris), sans les guillemets de la page.
# Ces valeurs ne sont pas secrètes : elles figurent dans chaque paquet publié.
# Les paramètres -IdentityName, -Publisher et -PublisherDisplayName du script sont prioritaires.
@{
    # Ligne « Package/Identity/Name », par exemple 12345Nads.NadsPDF
    IdentityName         = 'NadsDev.NadsPDF'

    # Ligne « Package/Identity/Publisher », commence par CN=, par exemple CN=1A2B3C4D-5E6F-...
    Publisher            = 'CN=4EA5CC12-0A40-4E87-BD63-CD40E93A4F8F'

    # Ligne « Package/Properties/PublisherDisplayName », votre nom d'éditeur, par exemple nads
    PublisherDisplayName = 'Nads Dev'
}
